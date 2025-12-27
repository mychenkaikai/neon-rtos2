/* RISC-V QEMU virt machine memory layout */
/* 兼容 riscv-rt 0.12 的链接脚本 */

MEMORY
{
    /* QEMU virt machine RAM starts at 0x80000000 */
    RAM : ORIGIN = 0x80000000, LENGTH = 128M
}

/* Region aliases for riscv-rt */
REGION_ALIAS("REGION_TEXT", RAM);
REGION_ALIAS("REGION_RODATA", RAM);
REGION_ALIAS("REGION_DATA", RAM);
REGION_ALIAS("REGION_BSS", RAM);
REGION_ALIAS("REGION_HEAP", RAM);
REGION_ALIAS("REGION_STACK", RAM);

/* Stack size - 64KB */
_stack_size = 64K;

/* Heap configuration */
_heap_size = 64K;

/* Provide symbols that might be needed */
PROVIDE(_stext = ORIGIN(RAM));
PROVIDE(_stack_start = ORIGIN(RAM) + LENGTH(RAM));
PROVIDE(_max_hart_id = 0);
PROVIDE(_hart_stack_size = 16K);
PROVIDE(_heap_size = 64K);

/* riscv-rt 0.12 需要的符号 */
PROVIDE(_mp_hook = default_mp_hook);
PROVIDE(__pre_init = default_pre_init);
PROVIDE(_setup_interrupts = default_setup_interrupts);
PROVIDE(_start_trap = default_start_trap);

/* 丢弃 .eh_frame 段，避免重定位问题 */
SECTIONS
{
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.eh_frame_hdr)
    }
}
