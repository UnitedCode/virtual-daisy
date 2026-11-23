#![no_std]
#![no_main]
use core::arch::asm;
#[repr(C)]
union Vector{stack:u32,handler:unsafe extern "C" fn()->!}
#[used]
#[unsafe(link_section=".vector_table")]
static VECTORS:[Vector;12]=[
 Vector{stack:0x20020000}, Vector{handler:Reset},
 Vector{handler:unexpected}, Vector{handler:unexpected}, Vector{handler:unexpected},
 Vector{handler:unexpected}, Vector{handler:unexpected}, Vector{handler:unexpected},
 Vector{handler:unexpected}, Vector{handler:unexpected}, Vector{handler:unexpected},
 Vector{handler:psr_oracle::restore_privilege},
];
extern "C" fn unexpected()->! {exit(2)}
unsafe fn call(op:u32,arg:u32)->u32{let mut r0=op;unsafe{asm!("bkpt #0xab",inout("r0")r0,in("r1")arg,options(nostack));}r0}
fn exit(code:u32)->!{let args=[0x20026,code];unsafe{call(0x20,args.as_ptr()as u32);}loop{}}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo)->!{exit(1)}
macro_rules! floating{($op:literal,$a:expr,$b:expr)=>{{let value:u32;let flags:u32;unsafe{asm!("vmov s1, {a}","vmov s2, {b}","vmsr fpscr, {zero}",$op,"vmov {value}, s0","vmrs {flags}, fpscr",a=in(reg)$a,b=in(reg)$b,zero=in(reg)0u32,value=out(reg)value,flags=out(reg)flags,out("s0")_,out("s1")_,out("s2")_,options(nostack));}(value,flags)}};}
macro_rules! controlled{($op:literal,$a:expr,$b:expr,$control:expr)=>{{let value:u32;let flags:u32;unsafe{asm!("vmov s1, {a}","vmov s2, {b}","vmsr fpscr, {control}",$op,"vmov {value}, s0","vmrs {flags}, fpscr",a=in(reg)$a,b=in(reg)$b,control=in(reg)$control,value=out(reg)value,flags=out(reg)flags,out("s0")_,out("s1")_,out("s2")_,options(nostack));}(value,flags)}};}
macro_rules! floating64{($op:literal,$a:expr,$b:expr)=>{{let lo:u32;let hi:u32;let flags:u32;unsafe{asm!("vmov d1, {al}, {ah}","vmov d2, {bl}, {bh}","vmsr fpscr, {zero}",$op,"vmov {lo}, {hi}, d0","vmrs {flags}, fpscr",al=in(reg)$a as u32,ah=in(reg)($a>>32)as u32,bl=in(reg)$b as u32,bh=in(reg)($b>>32)as u32,zero=in(reg)0u32,lo=out(reg)lo,hi=out(reg)hi,flags=out(reg)flags,out("d0")_,out("d1")_,out("d2")_,options(nostack));}(lo,hi,flags)}};}
macro_rules! controlled64{($op:literal,$a:expr,$b:expr,$control:expr)=>{{let lo:u32;let hi:u32;let flags:u32;unsafe{asm!("vmov d1, {al}, {ah}","vmov d2, {bl}, {bh}","vmsr fpscr, {control}",$op,"vmov {lo}, {hi}, d0","vmrs {flags}, fpscr",al=in(reg)$a as u32,ah=in(reg)($a>>32)as u32,bl=in(reg)$b as u32,bh=in(reg)($b>>32)as u32,control=in(reg)$control,lo=out(reg)lo,hi=out(reg)hi,flags=out(reg)flags,out("d0")_,out("d1")_,out("d2")_,options(nostack));}(lo,hi,flags)}};}
mod rounding_oracle;
mod arithmetic_rounding_oracle;
mod fused_oracle;
mod precision_oracle;
mod accumulate_oracle;
mod half_oracle;
mod fixed_oracle;
mod saturate_oracle;
mod psr_oracle;
mod qadd_oracle;
mod parallel_oracle;
mod dsp_multiply_oracle;
mod smull_oracle;
mod long_multiply_oracle;
#[unsafe(no_mangle)]
extern "C" fn Reset()->!{
 unsafe{core::ptr::write_volatile(0xe000ed88 as *mut u32,0x00f00000);asm!("dsb","isb",options(nostack));}
 let specials=[0u32,0x80000000,0x3f800000,0xbf800000,0x00800000,0x7f7fffff,1,0x80000001,0x7f800000,0x7fc12345];
 let mut output=[0u32;5*5*210];let mut cursor=0;let mut seed=0x56443312u32;
 for i in 0..210{
  let a=if i<10{specials[i]}else{seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;seed};
  seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;let b=if i<10{specials[(i*3+1)%10]}else{seed};
  let results=[floating!("vadd.f32 s0,s1,s2",a,b),floating!("vsub.f32 s0,s1,s2",a,b),floating!("vmul.f32 s0,s1,s2",a,b),floating!("vdiv.f32 s0,s1,s2",a,b),floating!("vsqrt.f32 s0,s1",a,b)];
  for (op,(value,flags)) in results.into_iter().enumerate(){output[cursor..cursor+5].copy_from_slice(&[a,b,op as u32,value,flags]);cursor+=5;}
 }
 let hex=b"0123456789abcdef";
 for record in output.chunks_exact(5) {
  let mut line=[0u8;42];let mut n=0;
  for word in record {for byte in word.to_le_bytes(){line[n]=hex[(byte>>4)as usize];line[n+1]=hex[(byte&15)as usize];n+=2;}}
  line[40]=b'\n';unsafe{call(4,line.as_ptr()as u32);}
 }
 let specials64=[0u64,0x8000000000000000,0x3ff0000000000000,0xbff0000000000000,0x0010000000000000,0x7fefffffffffffff,1,0x8000000000000001,0x7ff0000000000000,0xfff0000000000000,0x7ff8123456789abc,0x7ff0123456789abc];
 let mut output64=[0u32;8*5*344];let mut cursor=0;let mut seed=0x56443312a7f039c1u64;
 for i in 0..344 {
  let a=if i<144{specials64[i/12]}else{seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;seed};
  seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;let b=if i<144{specials64[i%12]}else{seed};
  let results=[floating64!("vadd.f64 d0,d1,d2",a,b),floating64!("vsub.f64 d0,d1,d2",a,b),floating64!("vmul.f64 d0,d1,d2",a,b),floating64!("vdiv.f64 d0,d1,d2",a,b),floating64!("vcmp.f64 d1,d2",a,b)];
  for (op,(lo,hi,flags)) in results.into_iter().enumerate(){output64[cursor..cursor+8].copy_from_slice(&[a as u32,(a>>32)as u32,b as u32,(b>>32)as u32,op as u32,lo,hi,flags]);cursor+=8;}
 }
 for record in output64.chunks_exact(8) {
  let mut line=[0u8;66];let mut n=0;
  for word in record {for byte in word.to_le_bytes(){line[n]=hex[(byte>>4)as usize];line[n+1]=hex[(byte&15)as usize];n+=2;}}
  line[64]=b'\n';unsafe{call(4,line.as_ptr()as u32);}
 }
 let special32=[0u32,0x80000000,0x3f800000,0xbf800000,0x00800000,0x7f7fffff,1,0x80000001,0x7f800000,0xff800000,0x7fc12345,0x7f812345];
 let mut seed=0x619375a1u32;
 for i in 0..344 {
  seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;
  let a=if i<144{special32[i/12]}else{seed};
  seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;
  let b=if i<144{special32[i%12]}else{seed};
  for control in [0u32,1<<24,1<<25,3<<24] {
   let results=[controlled!("vadd.f32 s0,s1,s2",a,b,control),controlled!("vsub.f32 s0,s1,s2",a,b,control),controlled!("vmul.f32 s0,s1,s2",a,b,control),controlled!("vdiv.f32 s0,s1,s2",a,b,control),controlled!("vsqrt.f32 s0,s1",a,b,control),controlled!("vcmp.f32 s1,s2",a,b,control),controlled!("vcmpe.f32 s1,s2",a,b,control)];
   for (op,(value,flags)) in results.into_iter().enumerate() {
    let record=[a,b,op as u32,control,value,flags];let mut line=[0u8;50];let mut n=0;
    for word in record {for byte in word.to_le_bytes(){line[n]=hex[(byte>>4)as usize];line[n+1]=hex[(byte&15)as usize];n+=2;}}
    line[48]=b'\n';unsafe{call(4,line.as_ptr()as u32);}
   }
  }
 }
 let minmax32=[0u32,0x80000000,0x3f800000,0xbf800000,0x00800000,0x7f7fffff,1,0x80000001,0x7f800000,0xff800000,0x7fc12345,0x7f812345,0xffc54321,0xff854321,0x007fffff,0x807fffff];
 let minmax64=[0u64,0x8000000000000000,0x3ff0000000000000,0xbff0000000000000,0x0010000000000000,0x7fefffffffffffff,1,0x8000000000000001,0x7ff0000000000000,0xfff0000000000000,0x7ff8123456789abc,0x7ff0123456789abc,0xfff8abcdef123456,0xfff0abcdef123456,0x000fffffffffffff,0x800fffffffffffff];
 let mut seed=0x71b251654f1d389au64;
 for i in 0..456 {
  seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;
  let a64=if i<256{minmax64[i/16]}else{seed};
  let a32=if i<256{minmax32[i/16]}else{seed as u32};
  seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;
  let b64=if i<256{minmax64[i%16]}else{seed};
  let b32=if i<256{minmax32[i%16]}else{seed as u32};
  for mode in [0u32,1<<24,1<<25,3<<24] {
   let control=mode|0xa0000010;
   for (op,(value,flags)) in [controlled!("vmaxnm.f32 s0,s1,s2",a32,b32,control),controlled!("vminnm.f32 s0,s1,s2",a32,b32,control)].into_iter().enumerate() {
    emit_minmax([32,a32,0,b32,0,op as u32,control,value,0,flags]);
   }
   for (op,(lo,hi,flags)) in [controlled64!("vmaxnm.f64 d0,d1,d2",a64,b64,control),controlled64!("vminnm.f64 d0,d1,d2",a64,b64,control)].into_iter().enumerate() {
    emit_minmax([64,a64 as u32,(a64>>32)as u32,b64 as u32,(b64>>32)as u32,op as u32,control,lo,hi,flags]);
   }
  }
 }
 rounding_oracle::run();
 arithmetic_rounding_oracle::run();
 fused_oracle::run();
 precision_oracle::run();
 accumulate_oracle::run();
 half_oracle::run();
 fixed_oracle::run();
 saturate_oracle::run();
 psr_oracle::run();
 qadd_oracle::run();
 parallel_oracle::run();
 dsp_multiply_oracle::run();
 smull_oracle::run();
 long_multiply_oracle::run();
 exit(0)
}

fn emit_minmax(record:[u32;10]) {
 let hex=b"0123456789abcdef";let mut line=[0u8;82];let mut n=0;
 for word in record {for byte in word.to_le_bytes(){line[n]=hex[(byte>>4)as usize];line[n+1]=hex[(byte&15)as usize];n+=2;}}
 line[80]=b'\n';unsafe{call(4,line.as_ptr()as u32);}
}
