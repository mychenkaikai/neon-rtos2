#[cfg(test)]
mod tests {
    use neon_rtos2::mem::allocator::init_heap;
    extern crate alloc;
    use alloc::boxed::Box;

    #[test]
    fn test_allocator() {
        init_heap();
        let b = Box::new([0u8; 100]);
        assert_eq!(b[0], 0);
        // println!("Allocation successful");
    }
}
