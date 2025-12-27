/* RISC-V QEMU virt machine memory layout */

MEMORY
{
    /* QEMU virt machine RAM starts at 0x80000000 */
    RAM : ORIGIN = 0x80000000, LENGTH = 128M
}

/* Entry point */
ENTRY(_start)

SECTIONS
{
    /* Code section */
    .text : ALIGN(4)
    {
        *(.text.init)
        *(.text .text.*)
    } > RAM

    /* Read-only data */
    .rodata : ALIGN(4)
    {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    } > RAM

    /* Initialized data */
    .data : ALIGN(4)
    {
        _sdata = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
        _edata = .;
    } > RAM

    /* BSS (uninitialized data) */
    .bss (NOLOAD) : ALIGN(4)
    {
        _sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        _ebss = .;
    } > RAM

    /* Stack */
    .stack (NOLOAD) : ALIGN(16)
    {
        _stack_start = .;
        . = . + 64K;
        _stack_end = .;
    } > RAM

    /* Heap */
    .heap (NOLOAD) : ALIGN(4)
    {
        _heap_start = .;
        . = . + 64K;
        _heap_end = .;
    } > RAM

    /* Discard debug sections */
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.eh_frame_hdr)
    }
}

/* Provide symbols for startup code */
PROVIDE(_stack_top = _stack_end);
PROVIDE(_heap_size = _heap_end - _heap_start);

