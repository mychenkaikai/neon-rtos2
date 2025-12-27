# =============================================================================
# RISC-V 上下文切换汇编代码
# =============================================================================
#
# 文件: context.s
# 说明: 实现 RISC-V 架构的上下文保存和恢复
#
# 寄存器约定 (RV32I):
#   x0  (zero) - 硬连线零
#   x1  (ra)   - 返回地址
#   x2  (sp)   - 栈指针
#   x3  (gp)   - 全局指针
#   x4  (tp)   - 线程指针
#   x5-x7      - 临时寄存器 (t0-t2)
#   x8  (s0)   - 保存寄存器/帧指针
#   x9  (s1)   - 保存寄存器
#   x10-x11    - 参数/返回值 (a0-a1)
#   x12-x17    - 参数 (a2-a7)
#   x18-x27    - 保存寄存器 (s2-s11)
#   x28-x31    - 临时寄存器 (t3-t6)
#
# 栈帧布局 (136 字节):
#   偏移    寄存器
#   0       (保留)
#   4       x1  (ra)
#   8       x3  (gp)
#   12      x4  (tp)
#   16      x5  (t0)
#   20      x6  (t1)
#   24      x7  (t2)
#   28      x8  (s0)
#   32      x9  (s1)
#   36      x10 (a0)
#   40      x11 (a1)
#   44      x12 (a2)
#   48      x13 (a3)
#   52      x14 (a4)
#   56      x15 (a5)
#   60      x16 (a6)
#   64      x17 (a7)
#   68      x18 (s2)
#   72      x19 (s3)
#   76      x20 (s4)
#   80      x21 (s5)
#   84      x22 (s6)
#   88      x23 (s7)
#   92      x24 (s8)
#   96      x25 (s9)
#   100     x26 (s10)
#   104     x27 (s11)
#   108     x28 (t3)
#   112     x29 (t4)
#   116     x30 (t5)
#   120     x31 (t6)
#   124     mepc
#   128     mstatus
#   132     (保留/对齐)
# =============================================================================

.section .text
.global context_switch
.global save_context
.global restore_context
.global trap_entry

# =============================================================================
# context_switch - 上下文切换
# =============================================================================
# 参数:
#   a0 - 当前任务栈指针的地址 (*mut usize)
#   a1 - 下一个任务的栈指针值 (usize)
#
# 功能:
#   1. 保存当前任务的上下文到栈
#   2. 保存当前栈指针到 *a0
#   3. 切换到新任务的栈
#   4. 恢复新任务的上下文
# =============================================================================
.align 4
context_switch:
    # 分配栈帧空间 (136 字节)
    addi    sp, sp, -136

    # 保存通用寄存器 (x1, x3-x31)
    sw      x1,  4(sp)      # ra
    sw      x3,  8(sp)      # gp
    sw      x4,  12(sp)     # tp
    sw      x5,  16(sp)     # t0
    sw      x6,  20(sp)     # t1
    sw      x7,  24(sp)     # t2
    sw      x8,  28(sp)     # s0
    sw      x9,  32(sp)     # s1
    sw      x10, 36(sp)     # a0
    sw      x11, 40(sp)     # a1
    sw      x12, 44(sp)     # a2
    sw      x13, 48(sp)     # a3
    sw      x14, 52(sp)     # a4
    sw      x15, 56(sp)     # a5
    sw      x16, 60(sp)     # a6
    sw      x17, 64(sp)     # a7
    sw      x18, 68(sp)     # s2
    sw      x19, 72(sp)     # s3
    sw      x20, 76(sp)     # s4
    sw      x21, 80(sp)     # s5
    sw      x22, 84(sp)     # s6
    sw      x23, 88(sp)     # s7
    sw      x24, 92(sp)     # s8
    sw      x25, 96(sp)     # s9
    sw      x26, 100(sp)    # s10
    sw      x27, 104(sp)    # s11
    sw      x28, 108(sp)    # t3
    sw      x29, 112(sp)    # t4
    sw      x30, 116(sp)    # t5
    sw      x31, 120(sp)    # t6

    # 保存 mepc 和 mstatus
    csrr    t0, mepc
    sw      t0, 124(sp)
    csrr    t0, mstatus
    sw      t0, 128(sp)

    # 保存当前栈指针到 *a0
    sw      sp, 0(a0)

    # 切换到新任务的栈
    mv      sp, a1

    # 恢复 mstatus 和 mepc
    lw      t0, 128(sp)
    csrw    mstatus, t0
    lw      t0, 124(sp)
    csrw    mepc, t0

    # 恢复通用寄存器
    lw      x1,  4(sp)      # ra
    lw      x3,  8(sp)      # gp
    lw      x4,  12(sp)     # tp
    lw      x5,  16(sp)     # t0
    lw      x6,  20(sp)     # t1
    lw      x7,  24(sp)     # t2
    lw      x8,  28(sp)     # s0
    lw      x9,  32(sp)     # s1
    lw      x10, 36(sp)     # a0
    lw      x11, 40(sp)     # a1
    lw      x12, 44(sp)     # a2
    lw      x13, 48(sp)     # a3
    lw      x14, 52(sp)     # a4
    lw      x15, 56(sp)     # a5
    lw      x16, 60(sp)     # a6
    lw      x17, 64(sp)     # a7
    lw      x18, 68(sp)     # s2
    lw      x19, 72(sp)     # s3
    lw      x20, 76(sp)     # s4
    lw      x21, 80(sp)     # s5
    lw      x22, 84(sp)     # s6
    lw      x23, 88(sp)     # s7
    lw      x24, 92(sp)     # s8
    lw      x25, 96(sp)     # s9
    lw      x26, 100(sp)    # s10
    lw      x27, 104(sp)    # s11
    lw      x28, 108(sp)    # t3
    lw      x29, 112(sp)    # t4
    lw      x30, 116(sp)    # t5
    lw      x31, 120(sp)    # t6

    # 释放栈帧
    addi    sp, sp, 136

    # 返回（对于新任务，会跳转到 mepc）
    mret

# =============================================================================
# trap_entry - 陷阱入口
# =============================================================================
# 所有异常和中断都从这里进入
# =============================================================================
.align 4
trap_entry:
    # 保存上下文
    addi    sp, sp, -136

    # 保存所有通用寄存器
    sw      x1,  4(sp)
    sw      x3,  8(sp)
    sw      x4,  12(sp)
    sw      x5,  16(sp)
    sw      x6,  20(sp)
    sw      x7,  24(sp)
    sw      x8,  28(sp)
    sw      x9,  32(sp)
    sw      x10, 36(sp)
    sw      x11, 40(sp)
    sw      x12, 44(sp)
    sw      x13, 48(sp)
    sw      x14, 52(sp)
    sw      x15, 56(sp)
    sw      x16, 60(sp)
    sw      x17, 64(sp)
    sw      x18, 68(sp)
    sw      x19, 72(sp)
    sw      x20, 76(sp)
    sw      x21, 80(sp)
    sw      x22, 84(sp)
    sw      x23, 88(sp)
    sw      x24, 92(sp)
    sw      x25, 96(sp)
    sw      x26, 100(sp)
    sw      x27, 104(sp)
    sw      x28, 108(sp)
    sw      x29, 112(sp)
    sw      x30, 116(sp)
    sw      x31, 120(sp)

    # 保存 mepc 和 mstatus
    csrr    t0, mepc
    sw      t0, 124(sp)
    csrr    t0, mstatus
    sw      t0, 128(sp)

    # 调用 Rust 陷阱处理函数
    # 参数: a0 = mcause, a1 = mepc, a2 = sp
    csrr    a0, mcause
    csrr    a1, mepc
    mv      a2, sp
    call    trap_handler

    # 如果 trap_handler 返回新的栈指针，使用它
    # 否则使用当前栈指针
    beqz    a0, 1f
    mv      sp, a0
1:

    # 恢复 mstatus 和 mepc
    lw      t0, 128(sp)
    csrw    mstatus, t0
    lw      t0, 124(sp)
    csrw    mepc, t0

    # 恢复所有通用寄存器
    lw      x1,  4(sp)
    lw      x3,  8(sp)
    lw      x4,  12(sp)
    lw      x5,  16(sp)
    lw      x6,  20(sp)
    lw      x7,  24(sp)
    lw      x8,  28(sp)
    lw      x9,  32(sp)
    lw      x10, 36(sp)
    lw      x11, 40(sp)
    lw      x12, 44(sp)
    lw      x13, 48(sp)
    lw      x14, 52(sp)
    lw      x15, 56(sp)
    lw      x16, 60(sp)
    lw      x17, 64(sp)
    lw      x18, 68(sp)
    lw      x19, 72(sp)
    lw      x20, 76(sp)
    lw      x21, 80(sp)
    lw      x22, 84(sp)
    lw      x23, 88(sp)
    lw      x24, 92(sp)
    lw      x25, 96(sp)
    lw      x26, 100(sp)
    lw      x27, 104(sp)
    lw      x28, 108(sp)
    lw      x29, 112(sp)
    lw      x30, 116(sp)
    lw      x31, 120(sp)

    # 释放栈帧
    addi    sp, sp, 136

    # 返回
    mret

# =============================================================================
# start_first_task_asm - 启动第一个任务
# =============================================================================
# 参数:
#   a0 - 第一个任务的栈指针
# =============================================================================
.global start_first_task_asm
.align 4
start_first_task_asm:
    # 设置栈指针
    mv      sp, a0

    # 恢复 mstatus 和 mepc
    lw      t0, 128(sp)
    csrw    mstatus, t0
    lw      t0, 124(sp)
    csrw    mepc, t0

    # 恢复所有通用寄存器
    lw      x1,  4(sp)
    lw      x3,  8(sp)
    lw      x4,  12(sp)
    lw      x5,  16(sp)
    lw      x6,  20(sp)
    lw      x7,  24(sp)
    lw      x8,  28(sp)
    lw      x9,  32(sp)
    lw      x10, 36(sp)
    lw      x11, 40(sp)
    lw      x12, 44(sp)
    lw      x13, 48(sp)
    lw      x14, 52(sp)
    lw      x15, 56(sp)
    lw      x16, 60(sp)
    lw      x17, 64(sp)
    lw      x18, 68(sp)
    lw      x19, 72(sp)
    lw      x20, 76(sp)
    lw      x21, 80(sp)
    lw      x22, 84(sp)
    lw      x23, 88(sp)
    lw      x24, 92(sp)
    lw      x25, 96(sp)
    lw      x26, 100(sp)
    lw      x27, 104(sp)
    lw      x28, 108(sp)
    lw      x29, 112(sp)
    lw      x30, 116(sp)
    lw      x31, 120(sp)

    # 释放栈帧
    addi    sp, sp, 136

    # 跳转到任务入口
    mret

