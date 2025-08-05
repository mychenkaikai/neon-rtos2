use core::cell::RefCell;
use critical_section::Mutex as CsMutex;

// 自定义包装器
pub struct CriticalData<T> {
    data: CsMutex<RefCell<T>>,
}

impl<T> CriticalData<T> {
    pub const fn new(value: T) -> Self {
        Self {
            data: CsMutex::new(RefCell::new(value)),
        }
    }
    
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        critical_section::with(|cs| {
            f(&mut *self.data.borrow(cs).borrow_mut())
        })
    }
    
    pub fn with_ref<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        critical_section::with(|cs| {
            f(&*self.data.borrow(cs).borrow())
        })
    }
}