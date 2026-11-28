ENTRY(Reset)
MEMORY { FLASH (rx) : ORIGIN = 0x08000000, LENGTH = 128K }
SECTIONS {
  .vector_table : { KEEP(*(.vector_table)) } > FLASH
  .text : { *(.text*) *(.rodata*) } > FLASH
  /DISCARD/ : { *(.ARM.exidx*) *(.ARM.extab*) }
}
