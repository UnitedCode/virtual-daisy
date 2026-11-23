//! Build the pinned Renode ARM-M core with a fetch/data transaction tag.
//! This is mandatory C API glue in the existing engine; cache behavior is Rust.
use super::{checked, Result};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
const INFRA: &str = "066a7f13c052215632d469c995c89aea37c573b1";
const TLIB: &str = "167decf9129758829762e582939128c0694e90d6";
const INFRA_SHA: &str = "8353496d3489e3801a45fd79b9281040e9d6b00f0c1615a5e77e9959b44e5b85";
const TLIB_SHA: &str = "8db4d1fe0a316eec5dcae83bd4d2757b5172332493d6fdecfd9d4b81cb7a77a3";
const TAG: &str =
    "    cpustate |= (READ_ACCESS_TYPE == ACCESS_INST_FETCH) ? (UINT64_C(1) << 63) : 0;";
// SyncTime may run inside a data callback, before the translated block ends.
// Bill only its completed prefix; the remaining declaration is billed later.
const PREFIX_TIME: &str = r#"
static uint32_t daisy_prefix_time_enabled = 1;
void tlib_daisy_set_prefix_time(uint32_t enabled) { daisy_prefix_time_enabled=enabled; }
EXC_VOID_1(tlib_daisy_set_prefix_time, uint32_t, enabled)
static void daisy_sync_instruction_prefix(void) {
    TranslationBlock *tb=cpu->current_tb;
    if(!daisy_prefix_time_enabled || !tb || !cpu->mem_io_pc ||
       cpu->v7m.exception_phase != V7M_EXCEPTION_PHASE_NONE ||
       !cpu->instructions_count_declaration || cpu->instructions_count_declaration > tb->icount) { return; }
    target_ulong data[TARGET_INSN_START_WORDS];
    int ordinal=cpu_get_data_for_pc(cpu,tb,cpu->mem_io_pc,true,data,false);
    if(ordinal<1) { return; }
    uint32_t completed=ordinal-1;
    uint32_t billed=tb->icount-cpu->instructions_count_declaration;
    if(completed>billed) {
        uint32_t delta=completed-billed;
        cpu->instructions_count_declaration-=delta;
        cpu->instructions_count_value+=delta;
        cpu->instructions_count_total_value+=delta;
    }
}
"#;
const PREFIX_RESTORE: &str = r#"
        int to_charge=executed_instructions;
        if(tb==cpu->current_tb && cpu->instructions_count_declaration && cpu->instructions_count_declaration<=tb->icount) {
            int billed=tb->icount-cpu->instructions_count_declaration;
            if(to_charge<billed) { tlib_abort("Daisy instruction prefix exceeds restored execution"); }
            to_charge-=billed;
        }
        cpu->instructions_count_value += to_charge;
        cpu->instructions_count_total_value += to_charge;
"#;
const MARKER:&str="\nuint32_t tlib_daisy_fetch_tag_version(void) { return 18; }\nEXC_INT_0(uint32_t, tlib_daisy_fetch_tag_version)\nvoid *daisy_fetch_state;\nuint32_t (*daisy_fetch_runtime)(void *, uint32_t, uint32_t);\nvoid tlib_daisy_set_runtime_fetch(void *state, void *callback) { daisy_fetch_state = state; daisy_fetch_runtime = callback; }\nEXC_VOID_2(tlib_daisy_set_runtime_fetch, void *, state, void *, callback)\nuint64_t tlib_daisy_memory_context(void) { target_ulong data[TARGET_INSN_START_WORDS]; if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_STACKING) { return UINT64_MAX-1; } if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_UNSTACKING) { return UINT64_MAX-2; } if(cpu->v7m.exception_phase == V7M_EXCEPTION_PHASE_VECTOR_READ) { return UINT64_MAX; } if(!cpu->current_tb || !cpu->mem_io_pc) { return 0; } if(cpu_get_data_for_pc(cpu, cpu->current_tb, cpu->mem_io_pc, true, data, false) < 0) { return 0; } return ((uint64_t)data[0] << 32) | cpu->regs[13]; }\nEXC_INT_0(uint64_t, tlib_daisy_memory_context)\n";
const RUNTIME: &str = r#"
    if(cpu->tb_restart_request == 0 && cpu->current_tb->size != 0 && daisy_fetch_runtime) {
        uint32_t result = daisy_fetch_runtime(daisy_fetch_state, cpu->current_tb->pc, cpu->current_tb->size);
        if(result == 3 || result > 4) { tlib_abort("Invalid Daisy runtime instruction fetch"); }
        if(result == 1) { tb_phys_invalidate(cpu->current_tb, -1); cpu->tb_restart_request = 1; }
        if(result == 2) { cpu->exit_request = 1; cpu->tb_restart_request = 1; }
        if(result == 4) {
            cpu->v7m.bus_fault_status |= 1U << BUS_FAULT_OFFSET; /* IBUSERR, no BFAR update */
            cpu->regs[15]=cpu->current_tb->pc;
            cpu->exception_index=EXCP_BUS_FAULT;
            cpu_loop_exit(cpu);
        }
    }
"#;
const BARRIER_EXPORT: &str = r#"
uint32_t (*daisy_barrier_callback)(void *, uint32_t);
void *daisy_barrier_state;
void tlib_daisy_set_barrier(void *state, void *callback) { daisy_barrier_state = state; daisy_barrier_callback = callback; }
EXC_VOID_2(tlib_daisy_set_barrier, void *, state, void *, callback)
"#;
const ALIGNMENT_EXPORT: &str = r#"
void *daisy_alignment_state;
uint32_t (*daisy_alignment_callback)(void *, uint32_t, uint32_t, uint32_t);
void tlib_daisy_set_alignment(void *state, void *callback) { daisy_alignment_state=state; daisy_alignment_callback=callback; }
EXC_VOID_2(tlib_daisy_set_alignment, void *, state, void *, callback)
"#;
const ALIGNMENT_HELPER: &str = r#"
extern void *daisy_alignment_state;
extern uint32_t (*daisy_alignment_callback)(void *, uint32_t, uint32_t, uint32_t);
void daisy_check_alignment(CPUState *env, uint32_t address, uint32_t alignment, uint32_t forced, void *retaddr) {
    if(!(address & (alignment-1)) || !daisy_alignment_callback) { return; }
    uint32_t result=daisy_alignment_callback(daisy_alignment_state,address,alignment,forced);
    if(result>1) { tlib_abort("Invalid Daisy alignment check"); }
    if(result) {
        extern void *global_retaddr;
        global_retaddr=retaddr;
        interrupt_current_translation_block_from_current_instruction(env,EXCP_UNALIGNED);
    }
}
void HELPER(daisy_alignment)(CPUState *env, uint32_t address, uint32_t alignment, uint32_t forced) {
    daisy_check_alignment(env,address,alignment,forced,GETPC());
}
"#;
const HALF_EXPORT: &str = r#"
uint32_t (*daisy_half_convert)(uint64_t, uint32_t, uint32_t, uint32_t, uint64_t *, uint32_t *);
void tlib_daisy_set_half(void *callback) { daisy_half_convert=callback; }
EXC_VOID_1(tlib_daisy_set_half, void *, callback)
uint32_t daisy_half_double_enabled=1;
void tlib_daisy_half_double(uint32_t enabled) { daisy_half_double_enabled=enabled; }
EXC_VOID_1(tlib_daisy_half_double, uint32_t, enabled)
"#;
const PSR_EXPORT: &str = r#"
uint64_t (*daisy_psr_access)(uint32_t,uint32_t,uint32_t,uint32_t,uint32_t);
void tlib_daisy_set_psr(void *callback) {daisy_psr_access=callback;}
EXC_VOID_1(tlib_daisy_set_psr,void *,callback)
uint32_t daisy_psr_enabled=1;
void tlib_daisy_psr_enabled(uint32_t enabled) {daisy_psr_enabled=enabled;}
EXC_VOID_1(tlib_daisy_psr_enabled,uint32_t,enabled)
"#;
const PSR_HELPERS: &str = r#"
extern uint64_t (*daisy_psr_access)(uint32_t,uint32_t,uint32_t,uint32_t,uint32_t);
extern uint32_t daisy_psr_enabled;
static uint32_t daisy_psr_service(CPUState *env,uint32_t reg,uint32_t value,uint32_t mask,uint32_t write) {
    if(!daisy_psr_access) {tlib_abort("Missing Rust PSR service");}
    uint64_t result=daisy_psr_access(xpsr_read(env),value,reg,mask,write);
    if(result>>32) {tlib_abort("Invalid Rust PSR request");}
    return (uint32_t)result;
}
uint32_t HELPER(daisy_v7m_mrs)(CPUState *env,uint32_t reg) {
    if(daisy_psr_enabled && reg<8 && reg!=4) {return daisy_psr_service(env,reg,0,0,0);}
    return HELPER(v7m_mrs)(env,reg);
}
void HELPER(daisy_v7m_msr)(CPUState *env,uint32_t reg,uint32_t value,uint32_t mask) {
    if(daisy_psr_enabled && reg<4) {xpsr_write(env,daisy_psr_service(env,reg,value,mask,1),0xf80f0000);}
    else {HELPER(v7m_msr)(env,reg,value);}
}
"#;
const HALF_HELPERS: &str = r#"
extern uint32_t (*daisy_half_convert)(uint64_t, uint32_t, uint32_t, uint32_t, uint64_t *, uint32_t *);
static uint64_t daisy_half_scalar(CPUState *env, uint64_t bits, uint32_t precision, uint32_t to_half) {
    uint32_t control=HELPER(vfp_get_fpscr)(env), flags=0;
    uint64_t result=0;
    if(!daisy_half_convert || daisy_half_convert(bits,precision,to_half,control,&result,&flags)!=0) {
        tlib_abort("Missing Rust half conversion kernel");
    }
    HELPER(vfp_set_fpscr)(env,control|flags);
    return result;
}
float64 HELPER(daisy_half_to_f64)(uint32_t bits, CPUState *env) {
    return make_float64(daisy_half_scalar(env,bits,64,0));
}
uint32_t HELPER(daisy_f64_to_half)(float64 bits, CPUState *env) {
    return (uint32_t)daisy_half_scalar(env,float64_val(bits),64,1);
}
float32 HELPER(daisy_half_to_f32)(uint32_t bits, CPUState *env) {
    return make_float32((uint32_t)daisy_half_scalar(env,bits,32,0));
}
uint32_t HELPER(daisy_f32_to_half)(float32 bits, CPUState *env) {
    return (uint32_t)daisy_half_scalar(env,float32_val(bits),32,1);
}
"#;
const HALF_PREPARE: &str = r#"
                            if(!(arm_feature(env,ARM_FEATURE_VFP_FP16) || arm_feature(env,ARM_FEATURE_VFP4))) { return 1; }
                            if(precision==DOUBLE_PRECISION) {
                                if(!arm_feature(env,ARM_FEATURE_VFP5) || !daisy_half_convert || !daisy_half_double_enabled || veclen!=0) { return 1; }
                                if(rn<6) {
                                    if(rd>=16) { return 1; }
                                    rm=((rm&15)<<1)|(rm>>4);
                                    gen_mov_F0_vreg(SINGLE_PRECISION,rm);
                                    break;
                                }
                                if(rm>=16) { return 1; }
                                rd=((rd&15)<<1)|(rd>>4);
                            }
                            /* Otherwise fall through */
"#;
const BARRIER_HELPER: &str = r#"
extern void *daisy_barrier_state;
extern uint32_t (*daisy_barrier_callback)(void *, uint32_t);
void HELPER(daisy_barrier)(CPUState *env, uint32_t kind) {
    if(daisy_barrier_callback) {
        uint32_t result = daisy_barrier_callback(daisy_barrier_state, kind);
        if(result > 1) { tlib_abort("Invalid Daisy memory barrier"); }
        if(result) {
            extern void *global_retaddr;
            global_retaddr = GETPC();
            interrupt_current_translation_block(env, EXCP_WATCHPOINT);
        }
    }
}
"#;

const EXCEPTION_EXPORT: &str = r#"
void *daisy_exception_state;
uint32_t (*daisy_exception_callback)(void *, uint32_t, uint32_t);
uint32_t (*daisy_late_interrupt)(uint32_t);
uint32_t daisy_tailchain_enabled=1;
void tlib_daisy_set_tailchain(uint32_t enabled) { daisy_tailchain_enabled=enabled; }
EXC_VOID_1(tlib_daisy_set_tailchain, uint32_t, enabled)
void tlib_daisy_set_late_interrupt(void *callback) { daisy_late_interrupt=callback; }
EXC_VOID_1(tlib_daisy_set_late_interrupt, void *, callback)
extern void daisy_reset_exception(void);
extern int daisy_vector_pending(void);
extern int daisy_entry_pending(void);
extern int daisy_return_pending(void);
void tlib_daisy_set_exception(void *state, void *callback) {
    daisy_exception_state=state; daisy_exception_callback=callback; daisy_reset_exception();
}
EXC_VOID_2(tlib_daisy_set_exception, void *, state, void *, callback)
uint32_t tlib_daisy_irq_can_preempt(void) {
    return (cpu->interrupt_request & CPU_INTERRUPT_HARD)
        && (daisy_vector_pending() || cpu->regs[15] < ARM_M_FNC_RETURN_MIN)
        && (!daisy_entry_pending() || daisy_late_interrupt)
        && (!daisy_return_pending() || daisy_tailchain_enabled) && !cpu->v7m.locked_up;
}
EXC_INT_0(uint32_t, tlib_daisy_irq_can_preempt)
"#;
const EXCEPTION_DECL: &str = r#"
extern void *daisy_exception_state;
extern uint32_t (*daisy_exception_callback)(void *, uint32_t, uint32_t);
extern uint32_t (*daisy_late_interrupt)(uint32_t);
extern uint32_t daisy_tailchain_enabled;
static struct { bool pending; uint32_t lr; int active; bool secure_target; bool lockup; } daisy_vector;
static struct { bool pending; uint32_t type; } daisy_return;
static uint32_t daisy_exception_bus(uint32_t address, uint32_t phase) {
    uint32_t result=daisy_exception_callback ? daisy_exception_callback(daisy_exception_state,address,phase):0;
    if(result>1) { tlib_abort("Invalid Daisy exception bus phase"); }
    return result;
}
"#;
const EXCEPTION_RESUME: &str = r#"
    bool lockup_after_exception_taken=false;
    if(daisy_vector.pending) {
        lr=daisy_vector.lr; active_exception=daisy_vector.active;
        secure_target=daisy_vector.secure_target; lockup_after_exception_taken=daisy_vector.lockup;
        daisy_vector.pending=false;
        goto daisy_vector_resume;
    }
"#;
const EXCEPTION_VECTOR: &str = r#"
        if(!env->v7m.has_trustzone && !lockup_after_exception_taken &&
           (env->interrupt_request & CPU_INTERRUPT_HARD) && daisy_late_interrupt) {
            uint32_t replacement=daisy_late_interrupt(env->v7m.exception);
            if(replacement<2 || replacement>511) { tlib_abort("Invalid Daisy late interrupt"); }
            env->v7m.exception=replacement; active_exception=replacement;
        }
        if(daisy_exception_bus(env->v7m.vecbase[env->secure]+env->v7m.exception*4,0)) {
            daisy_vector.pending=true; daisy_vector.lr=lr; daisy_vector.active=active_exception;
            daisy_vector.secure_target=secure_target; daisy_vector.lockup=lockup_after_exception_taken;
            env->v7m.exception_phase=V7M_EXCEPTION_PHASE_NONE;
            env->exception_index=EXCP_INTERRUPT;
            env->exit_request=1;
            cpu_loop_exit_without_hook(env);
        }
"#;
const EXCEPTION_RETURN: &str = r#"
daisy_return_resume:
    if(!env->v7m.has_trustzone && (type==0xfffffff1 || type==0xfffffff9 || type==0xfffffffd ||
                                 type==0xffffffe1 || type==0xffffffe9 || type==0xffffffed)) {
        if(daisy_tailchain_enabled && (env->interrupt_request & CPU_INTERRUPT_HARD)) {
            daisy_exception_bus(0,2);
            env->v7m.exception_return_tailchain=true;
            env->v7m.exception_return_type=type;
            env->exception_index=EXCP_IRQ;
            cpu_loop_exit(env);
        }
        uint32_t frame=(type&4) ? *pointer_psp(env):*pointer_msp(env);
        uint32_t bytes=(!(type&16) && !(env->v7m.fpccr[env->secure]&ARM_FPCCR_LSPACT_MASK)) ? 104:32;
        if(daisy_exception_bus(frame,0x100+bytes)) {
            daisy_return.pending=true; daisy_return.type=type;
            env->exception_index=EXCP_INTERRUPT;
            env->exit_request=1;
            cpu_loop_exit_without_hook(env);
        }
    }
"#;
const EXCEPTION_WRAPPERS: &str = r#"
int daisy_vector_pending(void) { return daisy_vector.pending || daisy_return.pending; }
int daisy_entry_pending(void) { return daisy_vector.pending; }
int daisy_return_pending(void) { return daisy_return.pending; }
void daisy_resume_vector(CPUState *env) {
    if(daisy_return.pending) { do_v7m_exception_exit(env); } else { do_interrupt_v7m(env); }
}
void daisy_reset_exception(void) { daisy_vector.pending=false; daisy_return.pending=false; daisy_tailchain_enabled=1; }
"#;

fn pinned_replace(code: &mut String, needle: &str, replacement: &str) -> Result<()> {
    if code.matches(needle).count() != 1 {
        return Err(format!("Pinned native adapter point changed: {needle}").into());
    }
    *code = code.replace(needle, replacement);
    Ok(())
}

fn archive(cache: &Path, name: &str, url: &str, sha: &str) -> Result<PathBuf> {
    super::source_archive::fetch(cache, name, sha, |path| {
        checked(
            Command::new("curl")
                .args([
                    "-fL",
                    "--connect-timeout",
                    "20",
                    "--max-time",
                    "120",
                    "--retry",
                    "2",
                    "--retry-max-time",
                    "180",
                    url,
                    "-o",
                ])
                .arg(path),
        )
    })
}

pub fn prepare(renode: &Path, directory: &Path, managed: &Path) -> Result<()> {
    let rid = virtual_daisy::host::runtime_id(env::consts::OS, env::consts::ARCH)?;
    let cache = env::var_os("VIRTUAL_DAISY_CORE_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("cpu-core"));
    fs::create_dir_all(&cache)?;
    let infra = archive(
        &cache,
        "infrastructure.tar.gz",
        &format!("https://codeload.github.com/renode/renode-infrastructure/tar.gz/{INFRA}"),
        INFRA_SHA,
    )?;
    let tlib = archive(
        &cache,
        "tlib.tar.gz",
        &format!("https://codeload.github.com/antmicro/tlib/tar.gz/{TLIB}"),
        TLIB_SHA,
    )?;
    let source = cache.join("source");
    let source_stamp = format!(
        "{:x}",
        Sha256::digest(format!("{INFRA}{TLIB}{}", include_str!("core.rs")))
    );
    if fs::read_to_string(source.join(".fetch-tag-v1"))
        .ok()
        .as_deref()
        != Some(&source_stamp)
    {
        if source.exists() {
            fs::remove_dir_all(&source)?;
        }
        fs::create_dir_all(&source)?;
        checked(
            Command::new("tar")
                .arg("-xzf")
                .arg(infra)
                .arg("-C")
                .arg(&source)
                .arg("--strip-components=1"),
        )?;
        let tlib_dir = source.join("src/Emulator/Cores/tlib");
        fs::create_dir_all(&tlib_dir)?;
        checked(
            Command::new("tar")
                .arg("-xzf")
                .arg(tlib)
                .arg("-C")
                .arg(&tlib_dir)
                .arg("--strip-components=1"),
        )?;
        let header = tlib_dir.join("include/softmmu_template.h");
        let mut original = fs::read_to_string(&header)?;
        let io_pc = "    cpu->mem_io_pc = (uintptr_t)retaddr;";
        if original.matches(io_pc).count() != 2 {
            return Err("Pinned read/write callback entries changed".into());
        }
        original = original.replace(io_pc, &format!("    global_retaddr = retaddr;\n{io_pc}"));
        let declaration = "uint32_t local_ntohl(uint32_t n);";
        pinned_replace(&mut original, declaration, &format!("{declaration}\n#ifdef TARGET_PROTO_ARM_M\nextern void daisy_check_alignment(CPUState *, uint32_t, uint32_t, uint32_t, void *);\n#endif"))?;
        let load = "    bool is_insn_fetch = (env->current_tb == NULL);";
        pinned_replace(&mut original, load, &format!("{load}\n#if defined(TARGET_PROTO_ARM_M) && SHIFT > 0 && !defined(SOFTMMU_CODE_ACCESS)\n    daisy_check_alignment(env,addr,SHIFT > 2 ? 4 : DATA_SIZE,SHIFT > 2,retaddr);\n#endif"))?;
        let store = "    target_phys_addr_t ioaddr;\n    target_ulong tlb_addr;\n    int index;\n    uintptr_t addend;";
        pinned_replace(&mut original, store, &format!("{store}\n#if defined(TARGET_PROTO_ARM_M) && SHIFT > 0\n    daisy_check_alignment(env,addr,SHIFT > 2 ? 4 : DATA_SIZE,SHIFT > 2,retaddr);\n#endif"))?;
        let needle="    uint64_t cpustate = cpu_get_state_for_memory_transaction(env, physaddr, READ_ACCESS_TYPE);";
        if original.matches(needle).count() != 1 {
            return Err("Pinned CPU fetch hook changed".into());
        }
        fs::write(
            header,
            original.replace(needle, &format!("{needle}\n{TAG}")),
        )?;
        let exports = tlib_dir.join("exports.c");
        let mut code = fs::read_to_string(&exports)?;
        let entry = "uint64_t tlib_get_executed_instructions()\n{";
        pinned_replace(
            &mut code,
            entry,
            &format!("{PREFIX_TIME}\n{entry}\n    daisy_sync_instruction_prefix();"),
        )?;
        code.push_str(MARKER);
        code.push_str(BARRIER_EXPORT);
        code.push_str(EXCEPTION_EXPORT);
        code.push_str(ALIGNMENT_EXPORT);
        code.push_str(HALF_EXPORT);
        code.push_str(PSR_EXPORT);
        let needle = "void tlib_reset()\n{";
        pinned_replace(
            &mut code,
            needle,
            &format!(
                "extern void daisy_reset_exception(void);\n{needle}\n    daisy_reset_exception();"
            ),
        )?;
        fs::write(exports, code)?;
        let translate = tlib_dir.join("arch/translate-all.c");
        let mut code = fs::read_to_string(&translate)?;
        pinned_replace(&mut code,
            "        cpu->instructions_count_value += executed_instructions;\n        cpu->instructions_count_total_value += executed_instructions;",
            PREFIX_RESTORE)?;
        fs::write(translate, code)?;
        let helper = tlib_dir.join("helper.c");
        let original = fs::read_to_string(&helper)?;
        let needle = "    return cpu->tb_restart_request;";
        if original.matches(needle).count() != 1 {
            return Err("Pinned runtime fetch hook changed".into());
        }
        fs::write(helper,format!("extern void *daisy_fetch_state;\nextern unsigned int (*daisy_fetch_runtime)(void *, unsigned int, unsigned int);\n{}",original.replace(needle,&format!("{RUNTIME}{needle}"))))?;
        let arm_helper = tlib_dir.join("arch/arm/helper.c");
        let mut code = fs::read_to_string(&arm_helper)?;
        // M7 requires eight-byte exception frames even after CCR writes of
        // zero. Keep the native stacking implementation independent.
        let reset = "    env->number_of_idau_regions = number_of_idau_regions;";
        pinned_replace(&mut code, reset, &format!("{reset}\n    if((id & 0xff00fff0U) == 0x4100c270U) {{ env->v7m.ccr[M_REG_NS] = 0x200U; }}"))?;
        let write_ccr = "void ccr_write(CPUState *env, uint32_t value, bool is_secure)\n{";
        pinned_replace(&mut code, write_ccr, &format!("{write_ccr}\n    if((env->cp15.c0_cpuid & 0xff00fff0U) == 0x4100c270U) {{ value = (value & 0x3011bU) | 0x200U; }}"))?;
        let read_ccr = "    uint32_t value = (common & V7M_CCR_NOT_BANKED_FIELDS) | (banked & ~V7M_CCR_NOT_BANKED_FIELDS);";
        pinned_replace(&mut code, read_ccr, &format!("{read_ccr}\n    if((env->cp15.c0_cpuid & 0xff00fff0U) == 0x4100c270U) {{ return (value & 0x3011bU) | 0x40200U; }}"))?;
        // PPB permissions are fixed even with the MPU disabled. Retain the
        // access's user MMU index, including privileged code's STRT/LDRT.
        let disabled =
            "        if((env->cp15.c1_sys & 1) == 0) {\n        /* MMU/MPU disabled.  */";
        let ppb = r#"#ifdef TARGET_PROTO_ARM_M
        if(!arm_feature(env, ARM_FEATURE_V8) && is_user && address >= 0xE0000000 && address <= 0xE00FFFFF) {
        bool user_stir = access_type == ACCESS_DATA_STORE && tlib_nvic_is_stir_address(address)
            && (env->v7m.ccr[is_secure] & FIELD_MASK(V7M_CCR, USERSETMPEND));
        *phys_ptr = address;
        *prot = user_stir ? PAGE_WRITE : 0;
        *page_size = 0; /* CCR changes must not leave a writable user page. */
        ret = user_stir ? TRANSLATE_SUCCESS : MPU_PERMISSION_FAULT;
    } else
#endif
"#;
        pinned_replace(&mut code, disabled, &format!("{ppb}{disabled}"))?;
        // Physical lazy-FP stores originate in a TCG helper rather than the
        // softmmu accessors. Preserve that helper's unwind PC so the existing
        // Rust instruction replay can resume without repeating completed stores.
        for name in ["fp_lsp", "fp_lsp_no_context"] {
            let entry = format!("void HELPER({name})(CPUState *env)\n{{");
            pinned_replace(&mut code,&entry,&format!("{entry}\n    extern void *global_retaddr;\n    global_retaddr=GETPC();\n    env->mem_io_pc=(uintptr_t)global_retaddr;"))?;
        }
        let div = "        case EXCP_DIV_0:";
        pinned_replace(&mut code, div, &format!("        case EXCP_UNALIGNED:\n            env->v7m.fault_status[env->secure] |= USAGE_FAULT_UNALIGNED;\n            v7m_raise_synchronous_exception(env, v7m_exception_number_with_security(env, ARMV7M_EXCP_USAGE, env->secure));\n            return;\n{div}"))?;
        let needle = "void do_v7m_exception_exit(CPUState *env)";
        pinned_replace(&mut code, needle, &format!("{EXCEPTION_DECL}\n{needle}"))?;
        pinned_replace(
            &mut code,
            "    bool lockup_after_exception_taken = false;",
            "    lockup_after_exception_taken = false;",
        )?;
        let needle = "    /* For exceptions we just mark as pending on the NVIC, and let that";
        pinned_replace(&mut code, needle, &format!("{EXCEPTION_RESUME}\n{needle}"))?;
        let needle="    while(true) {\n        env->v7m.exception_phase = V7M_EXCEPTION_PHASE_VECTOR_READ;";
        pinned_replace(
            &mut code,
            needle,
            &format!("daisy_vector_resume:\n{needle}\n{EXCEPTION_VECTOR}"),
        )?;
        let needle = "        int vector_fault = env->v7m.exception_phase_fault;";
        pinned_replace(
            &mut code,
            needle,
            &format!("        daisy_exception_bus(0,1);\n{needle}"),
        )?;
        let needle = "    int validation_fault = 0;";
        pinned_replace(&mut code, needle, &format!("{needle}\n    if(daisy_return.pending) {{ type=daisy_return.type; daisy_return.pending=false; goto daisy_return_resume; }}"))?;
        let needle = "    if(env->v7m.has_trustzone) {\n        /* Location of return stack type (Secure/Non-secure) */";
        pinned_replace(&mut code, needle, &format!("{EXCEPTION_RETURN}\n{needle}"))?;
        let needle = "    int unstacking_fault = env->v7m.exception_phase_fault;";
        pinned_replace(
            &mut code,
            needle,
            &format!("    daisy_exception_bus(0,2);\n{needle}"),
        )?;
        fs::write(
            arm_helper,
            format!("{code}\n{BARRIER_HELPER}\n{ALIGNMENT_HELPER}\n{EXCEPTION_WRAPPERS}\n{HALF_HELPERS}\n{PSR_HELPERS}"),
        )?;
        let cpu_header = tlib_dir.join("arch/arm/cpu.h");
        let mut code = fs::read_to_string(&cpu_header)?;
        pinned_replace(
            &mut code,
            "(env->QF << 27) |\n           (env->thumb << 24)",
            "(env->QF << 27) | (env->GE << 16) |\n           (env->thumb << 24)",
        )?;
        let psr_write =
            "static inline void xpsr_write(CPUState *env, uint32_t val, uint32_t mask)\n{";
        pinned_replace(
            &mut code,
            psr_write,
            &format!("{psr_write}\n    if(mask & CPSR_GE) {{env->GE=(val>>16)&15;}}"),
        )?;
        pinned_replace(
            &mut code,
            "#define EXCP_DIV_0",
            "#define EXCP_UNALIGNED      22 /* Data alignment UsageFault */\n#define EXCP_DIV_0",
        )?;
        fs::write(cpu_header, code)?;
        // Keep the native arithmetic independent. Repair its operand
        // representation and GE lane packing, rather than replacing its math.
        let packed = tlib_dir.join("arch/arm_common/op_addsub.h");
        let mut code = fs::read_to_string(&packed)?;
        let start = code
            .find("#ifdef PFX_U\n")
            .ok_or("Missing unsigned DSP helpers")?;
        let end = code[start..]
            .find("#ifdef PFX_SH\n")
            .ok_or("Missing signed halving helpers")?
            + start;
        let mut unsigned = code[start..end].to_owned();
        if unsigned.matches("(int32_t)((int16_t)").count() != 4 {
            return Err("Pinned unsigned DSP operand representations changed".into());
        }
        unsigned = unsigned
            .replace("(int32_t)((int16_t)a)", "(int32_t)a")
            .replace("(int32_t)((int16_t)b)", "(int32_t)b");
        code.replace_range(start..end, &unsigned);
        for helper in ["subaddx", "addsubx"] {
            let start = code
                .find(&format!("uint32_t HELPER(glue(PFX, {helper}))"))
                .ok_or("Missing exchange helper")?;
            let end = code[start..]
                .find("\n}\n")
                .ok_or("Missing exchange helper end")?
                + start
                + 3;
            let mut exchange = code[start..end].to_owned();
            // The top result belongs to GE[3:2], the bottom to GE[1:0].
            pinned_replace(
                &mut exchange,
                "    ge_flags |= (3 * ge);",
                "    ge_flags |= (3 * ge) << 2;",
            )?;
            let bottom = exchange
                .find("    res_bottom =")
                .ok_or("Missing exchange bottom lane")?;
            let (top, tail) = exchange.split_at(bottom);
            let mut tail = tail.to_owned();
            pinned_replace(
                &mut tail,
                "    ge_flags |= (3 * ge) << 2;",
                "    ge_flags |= (3 * ge);",
            )?;
            code.replace_range(start..end, &format!("{top}{tail}"));
        }
        fs::write(packed, code)?;
        let exec = tlib_dir.join("cpu-exec.c");
        let mut code = fs::read_to_string(&exec)?;
        let needle = "int cpu_exec(CPUState *env)";
        pinned_replace(&mut code,needle,&format!("extern int daisy_vector_pending(void);\nextern void daisy_resume_vector(CPUState *env);\n{needle}"))?;
        pinned_replace(
            &mut code,
            "    if(!cpu_has_work(env)) {",
            "    if(!cpu_has_work(env) && !daisy_vector_pending()) {",
        )?;
        let needle = "                interrupt_request = env->interrupt_request;";
        pinned_replace(&mut code,needle,&format!("                if(daisy_vector_pending()) {{ daisy_resume_vector(env); next_tb=0; }}\n{needle}"))?;
        fs::write(exec, code)?;
        let header = tlib_dir.join("arch/arm/helper.h");
        let code = fs::read_to_string(&header)?;
        let needle = "DEF_HELPER_1(clz, i32, i32)";
        if code.matches(needle).count() != 1 {
            return Err("Pinned barrier declaration changed".into());
        }
        fs::write(
            header,
            code.replace(
                needle,
                &format!("DEF_HELPER_4(daisy_alignment, void, env, i32, i32, i32)\nDEF_HELPER_2(daisy_barrier, void, env, i32)\nDEF_HELPER_2(daisy_half_to_f64, f64, i32, env)\nDEF_HELPER_2(daisy_f64_to_half, i32, f64, env)\nDEF_HELPER_2(daisy_half_to_f32, f32, i32, env)\nDEF_HELPER_2(daisy_f32_to_half, i32, f32, env)\nDEF_HELPER_2(daisy_v7m_mrs, i32, env, i32)\nDEF_HELPER_4(daisy_v7m_msr, void, env, i32, i32, i32)\n{needle}"),
            ),
        )?;
        let translate = tlib_dir.join("arch/arm/translate.c");
        let mut code = fs::read_to_string(&translate)?;
        pinned_replace(
            &mut code,
            "gen_helper_v7m_mrs(tmp, cpu_env, addr);",
            "gen_helper_daisy_v7m_mrs(tmp, cpu_env, addr);",
        )?;
        let msr="                                tmp = load_reg(s, rn);\n                                addr = tcg_const_i32(insn & 0xff);\n                                gen_helper_v7m_msr(cpu_env, addr, tmp);";
        pinned_replace(&mut code,msr,"                                tmp = load_reg(s, rn);\n                                addr = tcg_const_i32(insn & 0xff);\n                                TCGv_i32 daisy_mask=tcg_const_i32((insn>>10)&3);\n                                gen_helper_daisy_v7m_msr(cpu_env, addr, tmp, daisy_mask);\n                                tcg_temp_free_i32(daisy_mask);")?;
        pinned_replace(&mut code,"                             * UNPREDICTABLE if bit 8 is set (we choose to UNDEF)","                             * Scalar storage conversions use the verified Rust kernel.")?;
        let prepare="                            if(precision == DOUBLE_PRECISION ||\n                               !(arm_feature(env, ARM_FEATURE_VFP_FP16) || arm_feature(env, ARM_FEATURE_VFP4))) {\n                                return 1;\n                            }\n                            /* Otherwise fall through */";
        pinned_replace(&mut code, prepare, HALF_PREPARE)?;
        let from="                                    gen_helper_vfp_fcvt_f16_to_f32(cpu_F0s, tmp, cpu_env);";
        if code.matches(from).count() != 2 {
            return Err("Pinned half source conversions changed".into());
        }
        code=code.replace(from,"                                    if(precision==DOUBLE_PRECISION) { gen_helper_daisy_half_to_f64(cpu_F0d,tmp,cpu_env); }\n                                    else { gen_helper_daisy_half_to_f32(cpu_F0s,tmp,cpu_env); }");
        let to="                                    gen_helper_vfp_fcvt_f32_to_f16(tmp, cpu_F0s, cpu_env);";
        if code.matches(to).count() != 2 {
            return Err("Pinned half destination conversions changed".into());
        }
        code=code.replace(to,"                                    if(precision==DOUBLE_PRECISION) { gen_helper_daisy_f64_to_half(tmp,cpu_F0d,cpu_env); precision=SINGLE_PRECISION; }\n                                    else { gen_helper_daisy_f32_to_half(tmp,cpu_F0s,cpu_env); }");
        let include = "#include \"cpu.h\"";
        pinned_replace(&mut code,include,&format!("{include}\nextern uint32_t (*daisy_half_convert)(uint64_t,uint32_t,uint32_t,uint32_t,uint64_t *,uint32_t *);\nextern uint32_t daisy_half_double_enabled;\nextern void *daisy_fetch_state;\nextern uint32_t (*daisy_fetch_runtime)(void *, uint32_t, uint32_t);"))?;
        let page_check =
            "    if((base->pc - (base->tb->pc & TARGET_PAGE_MASK)) >= TARGET_PAGE_SIZE) {";
        pinned_replace(&mut code,page_check,&format!("    if(base->tb->icount==1 && daisy_fetch_runtime) {{\n        uint32_t policy=daisy_fetch_runtime(daisy_fetch_state,base->tb->pc,0);\n        if(policy!=0 && policy!=5) {{ tlib_abort(\"Invalid Daisy translation policy\"); }}\n        if(policy==5) {{ return 0; }}\n    }}\n{page_check}"))?;
        for entry in [
            "static void gen_load_exclusive(DisasContext *s, int rt, int rt2, TCGv addr, int size)\n{",
            "static void gen_store_exclusive(DisasContext *s, int rd, int rt, int rt2, TCGv addr, int size)\n{",
        ] {
            pinned_replace(&mut code,entry,&format!("{entry}\n#ifdef TARGET_PROTO_ARM_M\n    gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(size>2 ? 4:(1<<size)),tcg_const_i32(1));\n#endif"))?;
        }
        for name in ["ld", "st"] {
            let entry=format!("static inline void gen_vfp_{name}(DisasContext *s, enum arm_fp_precision precision, TCGv addr, int reg)\n{{");
            pinned_replace(&mut code,&entry,&format!("{entry}\n#ifdef TARGET_PROTO_ARM_M\n    gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(precision==HALF_PRECISION ? 2:4),tcg_const_i32(1));\n#endif"))?;
        }
        // Existing decoders own instruction semantics; only insert the mandatory
        // word-alignment check before their first memory transfer.
        for needle in [
            "                    if(insn & (1 << 20)) {\n                        /* ldrd */",
            "                    /* Load/store multiple.  */\n                    addr = load_reg(s, rn);",
            "            rn = (insn >> 8) & 0x7;\n            addr = load_reg(s, rn);\n            for(i = 0; i < 8; i++) {",
        ] {
            let replacement=if needle.contains("/* ldrd */") {
                format!("                    gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(4),tcg_const_i32(1));\n{needle}")
            } else {
                needle.replace("addr = load_reg(s, rn);","addr = load_reg(s, rn);\n            gen_helper_daisy_alignment(cpu_env,addr,tcg_const_i32(4),tcg_const_i32(1));")
            };
            pinned_replace(&mut code,needle,&replacement)?;
        }
        // A stalled second word restarts the instruction. Commit neither
        // destination until both loads complete, since Rn may alias Rt.
        pinned_replace(&mut code,
            "                        /* ldrd */\n                        tmp = gen_ld32(addr, context_to_mmu_index(s));\n                        store_reg(s, rs, tmp);\n                        tcg_gen_addi_i32(addr, addr, 4);\n                        tmp = gen_ld32(addr, context_to_mmu_index(s));\n                        store_reg(s, rd, tmp);",
            "                        /* ldrd: preserve aliased base across precise retries */\n                        tmp = gen_ld32(addr, context_to_mmu_index(s));\n                        tcg_gen_addi_i32(addr, addr, 4);\n                        tmp2 = gen_ld32(addr, context_to_mmu_index(s));\n                        store_reg(s, rs, tmp);\n                        store_reg(s, rd, tmp2);")?;
        for (needle,replacement) in [
            ("case 4: /* dsb */\n                case 5: /* dmb */", "case 4: /* dsb */\n                    gen_helper_daisy_barrier(cpu_env, tcg_const_i32(4));\n                    ARCH(7); gen_dxb(s); return;\n                case 5: /* dmb */\n                    gen_helper_daisy_barrier(cpu_env, tcg_const_i32(5));"),
            ("case 4: /* dsb */\n                                    case 5: /* dmb */", "case 4: /* dsb */\n                                        gen_helper_daisy_barrier(cpu_env, tcg_const_i32(4));\n                                        gen_dxb(s); break;\n                                    case 5: /* dmb */\n                                        gen_helper_daisy_barrier(cpu_env, tcg_const_i32(5));")
        ] {
            if code.matches(needle).count()!=1 {return Err("Pinned DSB/DMB decode changed".into());}
            code=code.replace(needle,replacement);
        }
        fs::write(translate, code)?;
        fs::write(source.join(".fetch-tag-v1"), &source_stamp)?;
    }
    let build = cache.join("build").join(rid);
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(virtual_daisy::host::tool_path(
            &source.join("src/Emulator/Cores"),
        )?)
        .arg("-B")
        .arg(virtual_daisy::host::tool_path(&build)?)
        .args(["-DTARGET_ARCH=arm-m", "-DCMAKE_BUILD_TYPE=Release"])
        .arg(format!("-DTLIB_COMMIT_SHA={TLIB}"));
    if cfg!(windows) {
        configure.args(["-G", "Ninja", "-DCMAKE_C_COMPILER=gcc"]);
    }
    checked(&mut configure)?;
    checked(
        Command::new("cmake")
            .arg("--build")
            .arg(&build)
            .args(["-j", "4"]),
    )?;
    let core = build.join("tlib/translate-arm-m-le.so");
    virtual_daisy::host::stage_runtime(
        &renode.join("platform-lib"),
        &managed.join("platform-lib"),
        rid,
        &core,
    )?;
    fs::write(directory.join("cpu-core.txt"),format!("infrastructure_commit={INFRA}\ntlib_commit={TLIB}\nfetch_tag_version=18\ncore_sha256={:x}\nsdk_version={}\nhalf_kernel_source_sha256={:x}\npsr_kernel_source_sha256={:x}\n",Sha256::digest(fs::read(core)?),env!("CARGO_PKG_VERSION"),Sha256::digest(include_bytes!("../cpu/src/executor/fp_half.rs")),Sha256::digest(include_bytes!("../cpu/src/core/status.rs"))))?;
    Ok(())
}
