ENTRY(probe_main)
PHDRS {
    vectors PT_LOAD FLAGS(4);
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
    flash PT_LOAD FLAGS(5);
    ramcode PT_LOAD FLAGS(5);
}
SECTIONS {
    .vectors 0x6000 : { KEEP(*(.vectors)) } :vectors
    .text 0xa000 : { *(.text*) *(.rodata*) } :text
    .data 0xe000 : { *(.data*) } :data
    .bss 0xf000 (NOLOAD) : { *(.bss*) *(COMMON) } :data
    .flashprobe 0x08010000 : { *(.flashprobe*) } :flash
    .axiprobe 0x24010000 : { *(.axiprobe*) } :ramcode
    /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) }
}
