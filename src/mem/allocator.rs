// 测试环境使用标准库分配器，不需要自定义分配器
#[cfg(test)]
pub fn init_heap() {
    // 测试环境下使用 std 的分配器，无需初始化
}

// 非测试环境使用 embedded-alloc
#[cfg(not(test))]
mod embedded_heap {
    use crate::config::HEAP_SIZE;
    use core::mem::MaybeUninit;
    use embedded_alloc::Heap;
    use spin::Once;
    use core::alloc::{GlobalAlloc, Layout};

    // 静态分配堆内存
    #[repr(align(8))]
    struct HeapStorage([MaybeUninit<u8>; HEAP_SIZE]);

    static mut HEAP_MEM: HeapStorage = HeapStorage([MaybeUninit::uninit(); HEAP_SIZE]);

    struct LazyHeap(Heap);

    unsafe impl GlobalAlloc for LazyHeap {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            super::init_heap();
            unsafe { self.0.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { self.0.dealloc(ptr, layout) }
        }
    }

    // 全局堆分配器
    #[global_allocator]
    static HEAP: LazyHeap = LazyHeap(Heap::empty());

    static HEAP_INIT: Once<()> = Once::new();

    // 初始化堆分配器
    pub fn init_heap() {
        HEAP_INIT.call_once(|| {
            unsafe {
                let heap_start = core::ptr::addr_of_mut!(HEAP_MEM).cast::<u8>() as usize;
                HEAP.0.init(heap_start, HEAP_SIZE);
            }
        });
    }
}

#[cfg(not(test))]
pub fn init_heap() {
    embedded_heap::init_heap();
}
