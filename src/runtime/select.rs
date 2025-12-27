//! # Select 宏
//!
//! 提供同时等待多个异步操作的能力，返回第一个完成的结果。
//!
//! ## 功能特性
//!
//! - 🚀 同时等待多个 Future
//! - ⚡ 返回第一个完成的结果
//! - 🔄 支持超时和取消
//! - 📦 零分配（栈上操作）
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use neon_rtos2::select;
//! use core::time::Duration;
//!
//! // 模拟异步接收
//! async fn recv() -> i32 { 42 }
//! // 模拟异步睡眠
//! async fn sleep(_dur: Duration) {}
//!
//! async fn example() {
//!     select! {
//!         data = recv() => {
//!             // println!("Received data: {:?}", data);
//!         }
//!         _ = sleep(Duration::from_secs(1)) => {
//!             // println!("Timeout!");
//!         }
//!     }
//! }
//! ```
//!
//! ## 实现原理
//!
//! `select!` 宏会：
//! 1. 将所有分支的 Future 包装到一个组合 Future 中
//! 2. 轮询所有 Future，直到其中一个完成
//! 3. 执行完成分支对应的处理代码
//! 4. 丢弃其他未完成的 Future

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

// ============================================================================
// Select Future 实现
// ============================================================================

/// 两个 Future 的选择器
///
/// 同时轮询两个 Future，返回第一个完成的结果
pub struct Select2<A, B> {
    a: Option<A>,
    b: Option<B>,
}

impl<A, B> Select2<A, B> {
    /// 创建新的 Select2
    pub fn new(a: A, b: B) -> Self {
        Self {
            a: Some(a),
            b: Some(b),
        }
    }
}

/// Select2 的结果
pub enum Either<A, B> {
    /// 第一个 Future 完成
    First(A),
    /// 第二个 Future 完成
    Second(B),
}

impl<A, B> Future for Select2<A, B>
where
    A: Future + Unpin,
    B: Future + Unpin,
{
    type Output = Either<A::Output, B::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 轮询第一个 Future
        if let Some(ref mut a) = self.a {
            if let Poll::Ready(result) = Pin::new(a).poll(cx) {
                self.a = None;
                return Poll::Ready(Either::First(result));
            }
        }

        // 轮询第二个 Future
        if let Some(ref mut b) = self.b {
            if let Poll::Ready(result) = Pin::new(b).poll(cx) {
                self.b = None;
                return Poll::Ready(Either::Second(result));
            }
        }

        Poll::Pending
    }
}

/// 三个 Future 的选择器
pub struct Select3<A, B, C> {
    a: Option<A>,
    b: Option<B>,
    c: Option<C>,
}

impl<A, B, C> Select3<A, B, C> {
    /// 创建新的 Select3
    pub fn new(a: A, b: B, c: C) -> Self {
        Self {
            a: Some(a),
            b: Some(b),
            c: Some(c),
        }
    }
}

/// Select3 的结果
pub enum Either3<A, B, C> {
    /// 第一个 Future 完成
    First(A),
    /// 第二个 Future 完成
    Second(B),
    /// 第三个 Future 完成
    Third(C),
}

impl<A, B, C> Future for Select3<A, B, C>
where
    A: Future + Unpin,
    B: Future + Unpin,
    C: Future + Unpin,
{
    type Output = Either3<A::Output, B::Output, C::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(ref mut a) = self.a {
            if let Poll::Ready(result) = Pin::new(a).poll(cx) {
                self.a = None;
                return Poll::Ready(Either3::First(result));
            }
        }

        if let Some(ref mut b) = self.b {
            if let Poll::Ready(result) = Pin::new(b).poll(cx) {
                self.b = None;
                return Poll::Ready(Either3::Second(result));
            }
        }

        if let Some(ref mut c) = self.c {
            if let Poll::Ready(result) = Pin::new(c).poll(cx) {
                self.c = None;
                return Poll::Ready(Either3::Third(result));
            }
        }

        Poll::Pending
    }
}

/// 四个 Future 的选择器
pub struct Select4<A, B, C, D> {
    a: Option<A>,
    b: Option<B>,
    c: Option<C>,
    d: Option<D>,
}

impl<A, B, C, D> Select4<A, B, C, D> {
    /// 创建新的 Select4
    pub fn new(a: A, b: B, c: C, d: D) -> Self {
        Self {
            a: Some(a),
            b: Some(b),
            c: Some(c),
            d: Some(d),
        }
    }
}

/// Select4 的结果
pub enum Either4<A, B, C, D> {
    First(A),
    Second(B),
    Third(C),
    Fourth(D),
}

impl<A, B, C, D> Future for Select4<A, B, C, D>
where
    A: Future + Unpin,
    B: Future + Unpin,
    C: Future + Unpin,
    D: Future + Unpin,
{
    type Output = Either4<A::Output, B::Output, C::Output, D::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(ref mut a) = self.a {
            if let Poll::Ready(result) = Pin::new(a).poll(cx) {
                self.a = None;
                return Poll::Ready(Either4::First(result));
            }
        }

        if let Some(ref mut b) = self.b {
            if let Poll::Ready(result) = Pin::new(b).poll(cx) {
                self.b = None;
                return Poll::Ready(Either4::Second(result));
            }
        }

        if let Some(ref mut c) = self.c {
            if let Poll::Ready(result) = Pin::new(c).poll(cx) {
                self.c = None;
                return Poll::Ready(Either4::Third(result));
            }
        }

        if let Some(ref mut d) = self.d {
            if let Poll::Ready(result) = Pin::new(d).poll(cx) {
                self.d = None;
                return Poll::Ready(Either4::Fourth(result));
            }
        }

        Poll::Pending
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 选择两个 Future 中第一个完成的
///
/// # 示例
///
/// ```rust,no_run
/// use neon_rtos2::runtime::select::{select2, Either};
///
/// async fn example() {
///     let future_a = async { 1 };
///     let future_b = async { 2 };
///     match select2(future_a, future_b).await {
///         Either::First(a) => {}, // println!("A completed: {:?}", a),
///         Either::Second(b) => {}, // println!("B completed: {:?}", b),
///     }
/// }
/// ```
pub fn select2<A, B>(a: A, b: B) -> Select2<A, B>
where
    A: Future + Unpin,
    B: Future + Unpin,
{
    Select2::new(a, b)
}

/// 选择三个 Future 中第一个完成的
pub fn select3<A, B, C>(a: A, b: B, c: C) -> Select3<A, B, C>
where
    A: Future + Unpin,
    B: Future + Unpin,
    C: Future + Unpin,
{
    Select3::new(a, b, c)
}

/// 选择四个 Future 中第一个完成的
pub fn select4<A, B, C, D>(a: A, b: B, c: C, d: D) -> Select4<A, B, C, D>
where
    A: Future + Unpin,
    B: Future + Unpin,
    C: Future + Unpin,
    D: Future + Unpin,
{
    Select4::new(a, b, c, d)
}

// ============================================================================
// Select 宏
// ============================================================================

#[doc(hidden)]
#[macro_export]
macro_rules! __select_future {
    // 递归终止条件：最后一个 Future
    ($pat:pat = $fut:expr => $expr:expr $(,)?) => {
        $fut
    };
    // 递归步骤：构建 Select2 链
    ($pat:pat = $fut:expr => $expr:expr, $($rest:tt)+) => {
        $crate::runtime::select::select2($fut, $crate::__select_future!($($rest)+))
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __select_match {
    // 递归终止条件：处理最后一个结果
    ($val:ident, $pat:pat = $fut:expr => $expr:expr $(,)?) => {
        {
            let $pat = $val;
            $expr
        }
    };
    // 递归步骤：解构 Either
    ($val:ident, $pat:pat = $fut:expr => $expr:expr, $($rest:tt)+) => {
        {
            use $crate::runtime::select::Either;
            match $val {
                Either::First($pat) => $expr,
                Either::Second(next_val) => {
                    $crate::__select_match!(next_val, $($rest)+)
                }
            }
        }
    };
}

/// 同时等待多个异步操作，返回第一个完成的结果
///
/// # 语法
///
/// ```rust,ignore
/// select! {
///     pattern1 = future1 => expression1,
///     pattern2 = future2 => expression2,
///     ...
/// }
/// ```
///
/// # 示例
///
/// ## 基本用法
///
/// ```rust,no_run
/// use neon_rtos2::select;
/// use core::time::Duration;
///
/// async fn recv() -> i32 { 42 }
/// async fn sleep(_: Duration) {}
///
/// async fn example() {
///     select! {
///         msg = recv() => {
///             // println!("Received: {:?}", msg);
///         }
///         _ = sleep(Duration::from_secs(1)) => {
///             // println!("Timeout!");
///         }
///     }
/// }
/// ```
///
/// ## 带返回值
///
/// ```rust,no_run
/// use neon_rtos2::select;
///
/// struct Data(i32);
/// struct Cmd(i32);
/// enum ProcessResult { Sensor(Data), Command(Cmd) }
///
/// async fn read_sensor() -> Data { Data(1) }
/// async fn recv_cmd() -> Cmd { Cmd(2) }
///
/// async fn example() {
///     let result = select! {
///         data = read_sensor() => ProcessResult::Sensor(data),
///         cmd = recv_cmd() => ProcessResult::Command(cmd),
///     };
/// }
/// ```
///
/// # 注意事项
///
/// - 所有 Future 必须实现 `Unpin`，或使用 `pin!` 宏固定
/// - 未完成的 Future 会被丢弃
/// - 分支按顺序检查，如果多个同时就绪，返回第一个
#[macro_export]
macro_rules! select {
    // 必须至少有两个分支 (单个分支直接 await 即可，但为了完整性也可以支持)
    // 这里我们支持 1+ 个分支
    
    // 单个分支的情况
    ($pat:pat = $fut:expr => $expr:expr $(,)?) => {
        {
            let $pat = $fut.await;
            $expr
        }
    };

    // 多个分支的情况
    ($($args:tt)+) => {{
        use $crate::runtime::select::{select2, Either};
        
        // 1. 构建 Future 链
        let future_chain = $crate::__select_future!($($args)+);
        
        // 2. 等待结果并匹配
        let result = future_chain.await;
        
        // 3. 递归匹配结果
        $crate::__select_match!(result, $($args)+)
    }};
}

// ============================================================================
// Biased Select（带优先级的选择）
// ============================================================================

/// 带优先级的选择器
///
/// 与 `select!` 不同，`select_biased!` 总是按顺序检查分支，
/// 优先返回靠前的分支结果。
///
/// # 示例
///
/// ```rust,no_run
/// use neon_rtos2::select_biased;
///
/// async fn recv_high() -> i32 { 1 }
/// async fn recv_low() -> i32 { 2 }
///
/// async fn example() {
///     // 高优先级消息总是优先处理
///     select_biased! {
///         msg = Box::pin(recv_high()) => {}, // handle_high_priority(msg),
///         msg = Box::pin(recv_low()) => {},  // handle_low_priority(msg),
///     }
/// }
/// ```
#[macro_export]
macro_rules! select_biased {
    // 与 select! 相同的实现，因为我们的实现本身就是有序的
    ($($args:tt)+) => {{
        $crate::select!($($args)+)
    }};
}

// ============================================================================
// Race（竞争）
// ============================================================================

/// 竞争多个相同类型的 Future
///
/// 与 `select` 不同，`race` 要求所有 Future 返回相同类型
pub struct Race<F, const N: usize> {
    futures: [Option<F>; N],
}

impl<F: Future + Unpin, const N: usize> Future for Race<F, N> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for slot in self.futures.iter_mut() {
            if let Some(fut) = slot {
                if let Poll::Ready(result) = Pin::new(fut).poll(cx) {
                    *slot = None;
                    return Poll::Ready(result);
                }
            }
        }
        Poll::Pending
    }
}

/// 竞争两个相同类型的 Future
///
/// # 示例
///
/// ```rust,no_run
/// use neon_rtos2::runtime::select::race2;
///
/// async fn fetch_a() -> i32 { 1 }
/// async fn fetch_b() -> i32 { 2 }
///
/// async fn example() {
///     let result = race2(fetch_a(), fetch_b()).await;
/// }
/// ```
pub fn race2<F: Future + Unpin>(a: F, b: F) -> Race<F, 2> {
    Race {
        futures: [Some(a), Some(b)],
    }
}

/// 竞争三个相同类型的 Future
pub fn race3<F: Future + Unpin>(a: F, b: F, c: F) -> Race<F, 3> {
    Race {
        futures: [Some(a), Some(b), Some(c)],
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::Waker;

    // 简单的立即完成 Future
    struct Ready<T>(Option<T>);

    impl<T> Ready<T> {
        fn new(value: T) -> Self {
            Self(Some(value))
        }
    }

    impl<T: Unpin> Future for Ready<T> {
        type Output = T;

        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready(self.0.take().unwrap())
        }
    }

    // 永不完成的 Future
    struct Pending;

    impl Future for Pending {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    impl Unpin for Pending {}

    // 可配置的 Future：可以是立即就绪或永远 Pending
    // 用于 race 测试，因为 race 要求所有 Future 类型相同
    enum MaybeReady<T> {
        Ready(Option<T>),
        Pending,
    }

    impl<T> MaybeReady<T> {
        fn ready(value: T) -> Self {
            Self::Ready(Some(value))
        }

        fn pending() -> Self {
            Self::Pending
        }
    }

    impl<T: Unpin> Future for MaybeReady<T> {
        type Output = T;

        fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            match &mut *self {
                MaybeReady::Ready(opt) => Poll::Ready(opt.take().unwrap()),
                MaybeReady::Pending => Poll::Pending,
            }
        }
    }

    #[test]
    fn test_select2_first_ready() {
        let mut select = Select2::new(Ready::new(42), Pending);
        
        // 创建一个虚拟的 waker
        let waker = unsafe { Waker::from_raw(core::task::RawWaker::new(
            core::ptr::null(),
            &core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {}),
        )) };
        static VTABLE: core::task::RawWakerVTable = core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {});
        
        let mut cx = Context::from_waker(&waker);
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either::First(value)) => assert_eq!(value, 42),
            _ => panic!("Expected First(42)"),
        }
    }

    #[test]
    fn test_select2_second_ready() {
        let mut select = Select2::new(Pending, Ready::new("hello"));
        
        let waker = unsafe { Waker::from_raw(core::task::RawWaker::new(
            core::ptr::null(),
            &core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {}),
        )) };
        static VTABLE: core::task::RawWakerVTable = core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {});
        
        let mut cx = Context::from_waker(&waker);
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either::Second(value)) => assert_eq!(value, "hello"),
            _ => panic!("Expected Second(\"hello\")"),
        }
    }

    #[test]
    fn test_select3() {
        let mut select = Select3::new(Pending, Ready::new(100), Pending);
        
        let waker = unsafe { Waker::from_raw(core::task::RawWaker::new(
            core::ptr::null(),
            &core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {}),
        )) };
        static VTABLE: core::task::RawWakerVTable = core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {});
        
        let mut cx = Context::from_waker(&waker);
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either3::Second(value)) => assert_eq!(value, 100),
            _ => panic!("Expected Second(100)"),
        }
    }

    #[test]
    fn test_race2() {
        let mut race = race2(Ready::new(1), Ready::new(2));
        
        let waker = unsafe { Waker::from_raw(core::task::RawWaker::new(
            core::ptr::null(),
            &core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {}),
        )) };
        static VTABLE: core::task::RawWakerVTable = core::task::RawWakerVTable::new(|_| core::task::RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {});
        
        let mut cx = Context::from_waker(&waker);
        
        // 第一个就绪的应该返回
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Ready(value) => assert_eq!(value, 1),
            _ => panic!("Expected Ready(1)"),
        }
    }

    // ========================================================================
    // 新增测试用例 (TASK-514)
    // ========================================================================

    /// 辅助函数：创建测试用的 Context
    fn create_test_context() -> Context<'static> {
        static VTABLE: core::task::RawWakerVTable = core::task::RawWakerVTable::new(
            |_| core::task::RawWaker::new(core::ptr::null(), &VTABLE),
            |_| {},
            |_| {},
            |_| {},
        );
        
        let waker = unsafe {
            Waker::from_raw(core::task::RawWaker::new(core::ptr::null(), &VTABLE))
        };
        
        // 使用 leak 来获得 'static 生命周期（仅用于测试）
        let waker_box = Box::new(waker);
        let waker_ref: &'static Waker = Box::leak(waker_box);
        Context::from_waker(waker_ref)
    }

    /// 测试 Select4：4 分支选择，最后一个分支就绪
    #[test]
    fn test_select4_fourth_ready() {
        let mut select = Select4::new(Pending, Pending, Pending, Ready::new("fourth"));
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either4::Fourth(value)) => assert_eq!(value, "fourth"),
            _ => panic!("Expected Fourth(\"fourth\")"),
        }
    }

    /// 测试 Select4：第一个分支就绪
    #[test]
    fn test_select4_first_ready() {
        let mut select = Select4::new(Ready::new(1), Pending, Pending, Pending);
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either4::First(value)) => assert_eq!(value, 1),
            _ => panic!("Expected First(1)"),
        }
    }

    /// 测试 Select4：第三个分支就绪
    #[test]
    fn test_select4_third_ready() {
        let mut select = Select4::new(Pending, Pending, Ready::new(333), Pending);
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either4::Third(value)) => assert_eq!(value, 333),
            _ => panic!("Expected Third(333)"),
        }
    }

    /// 测试所有 Future 都 Pending 的情况
    /// 
    /// 当所有分支都未就绪时，Select 应该返回 Poll::Pending
    #[test]
    fn test_select2_all_pending() {
        let mut select = Select2::<Pending, Pending>::new(Pending, Pending);
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Pending => {} // 预期结果
            Poll::Ready(_) => panic!("Expected Pending when all futures are pending"),
        }
    }

    /// 测试 Select3 所有分支都 Pending
    #[test]
    fn test_select3_all_pending() {
        let mut select = Select3::<Pending, Pending, Pending>::new(Pending, Pending, Pending);
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Pending => {} // 预期结果
            Poll::Ready(_) => panic!("Expected Pending when all futures are pending"),
        }
    }

    /// 测试 Select4 所有分支都 Pending
    #[test]
    fn test_select4_all_pending() {
        let mut select = Select4::<Pending, Pending, Pending, Pending>::new(
            Pending, Pending, Pending, Pending
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Pending => {} // 预期结果
            Poll::Ready(_) => panic!("Expected Pending when all futures are pending"),
        }
    }

    /// 测试 select! 宏支持 5 个分支
    #[test]
    fn test_select_macro_5_branches() {
        use core::pin::Pin;
        
        let f1 = Pending;
        let f2 = Pending;
        let f3 = Pending;
        let f4 = Pending;
        let f5 = Ready::new(5);
        
        let fut = async {
            crate::select! {
                _ = f1 => 0,
                _ = f2 => 0,
                _ = f3 => 0,
                _ = f4 => 0,
                v5 = f5 => v5,
            }
        };
        
        // 手动构建 Pin
        // 注意：在 no_std 测试中，我们使用 Box::pin
        let mut boxed = Box::pin(fut);
        let mut cx = create_test_context();
        
        match boxed.as_mut().poll(&mut cx) {
            Poll::Ready(val) => assert_eq!(val, 5),
            Poll::Pending => panic!("Should be ready"),
        }
    }

    /// 测试 select! 宏支持 3 个分支 (验证兼容性)
    #[test]
    fn test_select_macro_3_branches() {
        let f1 = Pending;
        let f2 = Ready::new(2);
        let f3 = Pending;
        
        let mut fut = Box::pin(async {
            crate::select! {
                _ = f1 => 0,
                v2 = f2 => v2,
                _ = f3 => 0,
            }
        });
        
        let mut cx = create_test_context();
        
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(val) => assert_eq!(val, 2),
            Poll::Pending => panic!("Should be ready"),
        }
    }

    /// 测试优先级：当多个 Future 同时就绪时，返回第一个
    /// 
    /// Select 按顺序轮询，所以当多个 Future 都就绪时，
    /// 应该返回排在前面的那个
    #[test]
    fn test_select2_priority_both_ready() {
        // 两个都就绪，应该返回第一个
        let mut select = Select2::new(Ready::new("first"), Ready::new("second"));
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either::First(value)) => assert_eq!(value, "first"),
            Poll::Ready(Either::Second(_)) => panic!("Should return First when both ready"),
            Poll::Pending => panic!("Should not be Pending"),
        }
    }

    /// 测试 Select3 优先级：三个都就绪时返回第一个
    #[test]
    fn test_select3_priority_all_ready() {
        let mut select = Select3::new(Ready::new(1), Ready::new(2), Ready::new(3));
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either3::First(value)) => assert_eq!(value, 1),
            _ => panic!("Should return First(1) when all ready"),
        }
    }

    /// 测试 Select4 优先级：四个都就绪时返回第一个
    #[test]
    fn test_select4_priority_all_ready() {
        let mut select = Select4::new(
            Ready::new("a"), 
            Ready::new("b"), 
            Ready::new("c"), 
            Ready::new("d")
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut select).poll(&mut cx) {
            Poll::Ready(Either4::First(value)) => assert_eq!(value, "a"),
            _ => panic!("Should return First(\"a\") when all ready"),
        }
    }

    /// 测试 Race3：三个相同类型 Future 的竞争，第一个就绪
    #[test]
    fn test_race3_first_ready() {
        let mut race = race3(
            MaybeReady::ready(100),
            MaybeReady::pending(),
            MaybeReady::pending(),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Ready(value) => assert_eq!(value, 100),
            Poll::Pending => panic!("Expected Ready(100)"),
        }
    }

    /// 测试 Race3：中间的 Future 就绪
    #[test]
    fn test_race3_middle_ready() {
        // 注意：由于 Pending 永不完成，只有 Ready 会返回
        // 但由于轮询顺序，第一个 Pending 会先被检查
        let mut race = race3(
            MaybeReady::pending(),
            MaybeReady::ready(200),
            MaybeReady::pending(),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Ready(value) => assert_eq!(value, 200),
            Poll::Pending => panic!("Expected Ready(200)"),
        }
    }

    /// 测试 Race3：最后一个 Future 就绪
    #[test]
    fn test_race3_last_ready() {
        let mut race = race3(
            MaybeReady::pending(),
            MaybeReady::pending(),
            MaybeReady::ready(300),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Ready(value) => assert_eq!(value, 300),
            Poll::Pending => panic!("Expected Ready(300)"),
        }
    }

    /// 测试 Race3：所有都就绪时返回第一个（优先级测试）
    #[test]
    fn test_race3_priority_all_ready() {
        let mut race = race3(
            MaybeReady::ready(1),
            MaybeReady::ready(2),
            MaybeReady::ready(3),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Ready(value) => assert_eq!(value, 1),
            Poll::Pending => panic!("Expected Ready(1)"),
        }
    }

    /// 测试 Race3 所有 Pending
    #[test]
    fn test_race3_all_pending() {
        let mut race: Race<MaybeReady<i32>, 3> = race3(
            MaybeReady::pending(),
            MaybeReady::pending(),
            MaybeReady::pending(),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Pending => {} // 预期结果
            Poll::Ready(_) => panic!("Expected Pending when all futures are pending"),
        }
    }

    /// 测试 Race2：第二个就绪
    #[test]
    fn test_race2_second_ready() {
        let mut race = race2(
            MaybeReady::pending(),
            MaybeReady::ready(42),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Ready(value) => assert_eq!(value, 42),
            Poll::Pending => panic!("Expected Ready(42)"),
        }
    }

    /// 测试 Race2 所有 Pending
    #[test]
    fn test_race2_all_pending() {
        let mut race: Race<MaybeReady<i32>, 2> = race2(
            MaybeReady::pending(),
            MaybeReady::pending(),
        );
        let mut cx = create_test_context();
        
        match Pin::new(&mut race).poll(&mut cx) {
            Poll::Pending => {} // 预期结果
            Poll::Ready(_) => panic!("Expected Pending when all futures are pending"),
        }
    }
}

