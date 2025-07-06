.syntax unified
.cpu cortex-m4
.thumb

.global PendSV_Handler
.type PendSV_Handler, %function

PendSV_Handler:
    cpsid i                  @ 禁用中断
    mrs r0, psp
    stmdb r0!, {r4-r11}
    bl task_switch_context
    ldmia r0!, {r4-r11}
    msr psp, r0
    mov lr, #0xFFFFFFFD
    cpsie i
    bx lr

.size PendSV_Handler, .-PendSV_Handler