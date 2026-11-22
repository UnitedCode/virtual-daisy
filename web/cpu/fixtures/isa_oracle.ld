ENTRY(Reset)
MEMORY {
 FLASH (rx) : ORIGIN = 0, LENGTH = 128K
 RAM (rwx) : ORIGIN = 0x20000000, LENGTH = 128K
}
SECTIONS {
 .vector_table : {KEEP(*(.vector_table))} > FLASH
 .text : {* (.text*) * (.rodata*)} > FLASH
 .data : {* (.data*)} > RAM AT> FLASH
 .bss (NOLOAD) : {* (.bss*)} > RAM
 /DISCARD/ : {*(.ARM.exidx*) *(.ARM.extab*)}
}
