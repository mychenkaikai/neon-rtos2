.syntax unified
.cpu cortex-m3
.thumb

.global PendSV_Handler
.type PendSV_Handler, %function

@ 外部变量：第一次切换标志
.extern FIRST_SWITCH

PendSV_Handler:
    cpsid i                     @ 禁用中断
    
    @ 获取当前 PSP
    mrs r0, psp
    
    @ 保存 r4-r11 到当前任务栈
    @ 注意：第一次切换时也会保存，但 task_switch_context 会忽略
    stmdb r0!, {r4-r11}
    
    @ 调用 Rust 函数进行任务切换
    @ 参数 r0 = 当前栈指针（已保存 r4-r11）
    @ 返回 r0 = 下一个任务的栈指针
    bl task_switch_context
    
    @ 从新任务栈恢复 r4-r11
    ldmia r0!, {r4-r11}
    
    @ 设置新的 PSP
    msr psp, r0
    
    @ 设置 LR 为 0xFFFFFFFD：
    @ - 返回到线程模式
    @ - 使用 PSP
    @ - 不使用 FPU
    mov lr, #0xFFFFFFFD
    
    @ 启用中断
    cpsie i
    
    @ 返回，硬件会自动从 PSP 恢复 r0-r3, r12, lr, pc, xpsr
    bx lr

.size PendSV_Handler, .-PendSV_Handler