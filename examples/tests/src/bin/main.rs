#![no_std]
#![no_main]

use core::panic::PanicInfo;
use cortex_m::Peripherals;
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m_rt::entry;
use neon_rtos2::schedule::Scheduler;
use neon_rtos2::task::Task;
use neon_rtos2::utils::kernel_init;
use neon_rtos2::timer::{Timer, Delay};
use neon_rtos2::log::{log_write, LogLevel, set_log_level, get_log_level};
use neon_rtos2::{info, error, warn, debug, trace};

const SYST_FREQ: u32 = 1000;
const SYS_CLOCK: u32 = 12_000_000;
const SYST_RELOAD: u32 = SYS_CLOCK / SYST_FREQ;

// 测试结果记录
#[derive(Copy, Clone)]
struct TestResult {
    name: &'static str,
    passed: bool,
}

// 全局测试状态
static mut TEST_RESULTS: [Option<TestResult>; 32] = [None; 32];
static mut TEST_COUNT: usize = 0;
static mut TESTS_COMPLETED: bool = false;

// 测试用例类型
type TestFn = fn() -> bool;

// 测试用例结构
#[derive(Clone)]
struct TestCase {
    name: &'static str,
    test_fn: TestFn,
}

// 添加测试结果
fn report_test_result(name: &'static str, passed: bool) {
    unsafe {
        if TEST_COUNT < 32 {
            TEST_RESULTS[TEST_COUNT] = Some(TestResult { name, passed });
            TEST_COUNT += 1;
        }
    }
}

// 运行单个测试
fn run_test(test: TestCase) {
    info!("运行测试: {}", test.name);
    let result = (test.test_fn)();
    report_test_result(test.name, result);
    info!("测试 {} {}", test.name, if result { "通过" } else { "失败" });
}

// 示例测试：互斥量测试
fn test_mutex() -> bool {
    use neon_rtos2::mutex::Mutex;
    
    let mutex = Mutex::new();
    

    true
}

// 示例测试：定时器测试
fn test_timer() -> bool {
    let mut timer = Timer::new(100);
    timer.start();
    assert!(timer.is_running());
    timer.stop();
    assert!(!timer.is_running());


    true
}

// 测试任务函数
fn test_task(_: usize) {
    use cortex_m_semihosting::debug;
    info!("开始执行测试套件");
    
    // 测试用例注册
    let tests = [
        TestCase { name: "mutex_test", test_fn: test_mutex },
        TestCase { name: "timer_test", test_fn: test_timer },
        // 添加更多测试...
    ];
    
    // 运行所有测试
    for test in &tests {
        run_test(test.clone());
    }
    
    // 报告结果
    info!("测试完成，结果汇总:");
    unsafe {
        for i in 0..TEST_COUNT {
            if let Some(result) = &TEST_RESULTS[i] {
                info!("{}: {}", result.name, if result.passed { "通过" } else { "失败" });
            }
        }
        TESTS_COMPLETED = true;
    }
    debug::exit(debug::EXIT_SUCCESS);
    // 测试完成，可以在这里添加结束逻辑
    loop {
        Delay::delay(1000);
    }
}

// 监控测试进度的任务
fn monitor_task(_: usize) {
    loop {
        unsafe {
            if TESTS_COMPLETED {
                info!("所有测试已完成");
                break;
            }
        }
        Delay::delay(500);
    }

    loop {
        Delay::delay(1000);
    }
}

#[entry]
fn main() -> ! {
    // 系统初始化
    kernel_init();
    set_log_level(LogLevel::Info);
    info!("测试框架初始化");
    
    // 初始化SysTick
    let p = Peripherals::take().unwrap();
    let mut syst = p.SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(SYST_RELOAD); 
    syst.enable_counter();
    syst.enable_interrupt();
    
    // 创建测试任务
    Task::new("test_task", test_task);
    Task::new("monitor", monitor_task);
    
    // 启动调度器
    info!("开始测试");
    Scheduler::start();
    
    loop {}
}
#[cfg(not(test))]
// 添加panic处理
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("测试失败: {:?}", info);
    loop {}
}