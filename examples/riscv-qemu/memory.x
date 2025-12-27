/* RISC-V QEMU virt machine memory layout */
/* 兼容 riscv-rt 的链接脚本 */

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
