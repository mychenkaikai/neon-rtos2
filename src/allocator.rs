use crate::config::HEAP_SIZE;
use core::mem::MaybeUninit;
use embedded_alloc::Heap as Heap;

// 静态分配堆内存
static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
// 全局堆分配器
#[global_allocator]
static HEAP: Heap = Heap::empty();

// 初始化堆分配器
pub fn init_heap() {
    unsafe {
        let heap_start = core::ptr::addr_of_mut!(HEAP_MEM).cast::<u8>() as usize;
        let heap_end = heap_start + HEAP_SIZE;
        HEAP.init(heap_start, heap_end);
    }
}
