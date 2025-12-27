use neon_rtos2::{kernel::task::Task, utils::kernel_init, kernel::scheduler::Scheduler, sync::event::Event};
use serial_test::serial;

#[test]
#[serial]
fn test_basic_task_operations() {
    // 初始化内核
    kernel_init();
    
    // 创建测试任务
    let task1 = Task::new("task1", |_| {}).unwrap();
    let task2 = Task::new("task2", |_| {}).unwrap();
    
    // 启动调度器
    Scheduler::start();
    
    // 验证任务创建成功并且状态正确
    assert_eq!(task1.get_name(), "task1");
    assert_eq!(task2.get_name(), "task2");
    
    // 验证调度器正常工作 - 一个任务应该处于运行状态
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task1.get_taskid() || 
            current_task.get_taskid() == task2.get_taskid());
}

#[test]
#[serial]
fn test_task_switching() {
    // 初始化内核
    kernel_init();
    
    // 创建测试任务
    Task::new("switch_task1", |_| {}).unwrap();
    Task::new("switch_task2", |_| {}).unwrap();
    
    // 启动调度器
    Scheduler::start();
    
    // 获取当前任务ID
    let task_id_before = Scheduler::get_current_task().get_taskid();
    
    // 执行任务切换
    Scheduler::task_switch();
    
    // 获取切换后的任务ID
    let task_id_after = Scheduler::get_current_task().get_taskid();
    
    // 验证任务已切换（任务ID应不同）
    assert_ne!(task_id_before, task_id_after);
}

#[test]
fn test_multiple_tasks() {
    // 初始化内核
    kernel_init();
    
    // 创建多个测试任务
    let task1 = Task::new("multi_task1", |_| {}).unwrap();
    let task2 = Task::new("multi_task2", |_| {}).unwrap();
    let task3 = Task::new("multi_task3", |_| {}).unwrap();
    
    // 启动调度器
    Scheduler::start();
    
    // 验证任务创建成功
    assert_eq!(task1.get_name(), "multi_task1");
    assert_eq!(task2.get_name(), "multi_task2");
    assert_eq!(task3.get_name(), "multi_task3");
    
    // 测试任务遍历功能
    let mut count = 0;
    Task::for_each(|_, _| {
        count += 1;
    });
    
    // 应该至少有3个任务（可能还有空闲任务）
    assert!(count >= 3);
} 
#[test]
//测试调度一整个循环
fn test_schedule_loop() {
    // 初始化内核
    kernel_init();
    
    // 创建10个测试任务
    let task1 = Task::new("schedule_loop_task1", |_| {}).unwrap();
    let task2 = Task::new("schedule_loop_task2", |_| {}).unwrap();
    let task3 = Task::new("schedule_loop_task3", |_| {}).unwrap();
    let task4 = Task::new("schedule_loop_task4", |_| {}).unwrap();
    let task5 = Task::new("schedule_loop_task5", |_| {}).unwrap();
    let task6 = Task::new("schedule_loop_task6", |_| {}).unwrap();
    let task7 = Task::new("schedule_loop_task7", |_| {}).unwrap();
    let task8 = Task::new("schedule_loop_task8", |_| {}).unwrap();
    let task9 = Task::new("schedule_loop_task9", |_| {}).unwrap();
    let task10 = Task::new("schedule_loop_task10", |_| {}).unwrap();
    // 启动调度器
    Scheduler::start();
    
    // 验证任务创建成功
    assert_eq!(task1.get_name(), "schedule_loop_task1");
    assert_eq!(task2.get_name(), "schedule_loop_task2");
    assert_eq!(task3.get_name(), "schedule_loop_task3");
    assert_eq!(task4.get_name(), "schedule_loop_task4");
    assert_eq!(task5.get_name(), "schedule_loop_task5");
    assert_eq!(task6.get_name(), "schedule_loop_task6");
    assert_eq!(task7.get_name(), "schedule_loop_task7");
    assert_eq!(task8.get_name(), "schedule_loop_task8");
    assert_eq!(task9.get_name(), "schedule_loop_task9");
    assert_eq!(task10.get_name(), "schedule_loop_task10");
    // 验证调度器正常工作,应该是第一个任务
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task1.get_taskid());

    // 验证调度器正常工作,应该是第二个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task2.get_taskid());

    // 验证调度器正常工作,应该是第三个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task3.get_taskid());

    // 验证调度器正常工作,应该是第四个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task4.get_taskid());

    // 验证调度器正常工作,应该是第五个任务
    Scheduler::task_switch();   
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task5.get_taskid());

    // 验证调度器正常工作,应该是第六个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task6.get_taskid());

    // 验证调度器正常工作,应该是第七个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task7.get_taskid());

    // 验证调度器正常工作,应该是第八个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task8.get_taskid());

    // 验证调度器正常工作,应该是第九个任务  
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task9.get_taskid());

    // 验证调度器正常工作,应该是第十个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert_eq!(current_task.get_taskid(), task10.get_taskid());  

    // 验证调度器正常工作,应该是第一个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task1.get_taskid());
    
}

#[test]
//测试任务调度中间,出现某个任务阻塞的逻辑,看调度器是否能正常调度
fn test_schedule_block() {
    // 初始化内核
    kernel_init();
    
    // 创建10个测试任务
    let task1 = Task::new("schedule_block_task1", |_| {}).unwrap();
    let task2 = Task::new("schedule_block_task2", |_| {}).unwrap();
    let mut task3 = Task::new("schedule_block_task3", |_| {}).unwrap();
    let task4 = Task::new("schedule_block_task4", |_| {}).unwrap();
    let task5 = Task::new("schedule_block_task5", |_| {}).unwrap();
    let task6 = Task::new("schedule_block_task6", |_| {}).unwrap();
    let task7 = Task::new("schedule_block_task7", |_| {}).unwrap();
    let task8 = Task::new("schedule_block_task8", |_| {}).unwrap();
    let task9 = Task::new("schedule_block_task9", |_| {}).unwrap();
    let task10 = Task::new("schedule_block_task10", |_| {}).unwrap();

    // 启动调度器
    Scheduler::start(); 

    // 验证任务创建成功
    assert_eq!(task1.get_name(), "schedule_block_task1");
    assert_eq!(task2.get_name(), "schedule_block_task2");
    assert_eq!(task3.get_name(), "schedule_block_task3");
    assert_eq!(task4.get_name(), "schedule_block_task4");
    assert_eq!(task5.get_name(), "schedule_block_task5");
    assert_eq!(task6.get_name(), "schedule_block_task6");
    assert_eq!(task7.get_name(), "schedule_block_task7");
    assert_eq!(task8.get_name(), "schedule_block_task8");
    assert_eq!(task9.get_name(), "schedule_block_task9");
    assert_eq!(task10.get_name(), "schedule_block_task10");

    // 验证调度器正常工作,应该是第一个任务
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task1.get_taskid());

    // 验证调度器正常工作,应该是第二个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task2.get_taskid());
    //阻塞第三个任务
    task3.block(Event::Timer(0));



    // 验证调度器正常工作,应该是第四个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task4.get_taskid());

    // 验证调度器正常工作,应该是第五个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task5.get_taskid());

    // 验证调度器正常工作,应该是第六个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task6.get_taskid());

    // 验证调度器正常工作,应该是第七个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task7.get_taskid());

    // 验证调度器正常工作,应该是第八个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task8.get_taskid());

    // 验证调度器正常工作,应该是第九个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task9.get_taskid());

    // 验证调度器正常工作,应该是第十个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task10.get_taskid());

    // 验证调度器正常工作,应该是第一个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task1.get_taskid());

    // 验证调度器正常工作,应该是第二个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task2.get_taskid());

    //唤醒第三个任务
    task3.ready();

    // 验证调度器正常工作,应该是第三个任务
    Scheduler::task_switch();
    let current_task = Scheduler::get_current_task();
    assert!(current_task.get_taskid() == task3.get_taskid());
}
