//! Process management syscalls
use core::mem::size_of;
// use core::ptr::write;

use crate::{
    config::MAX_SYSCALL_NUM,
    mm::translated_byte_buffer,
    task::{
        change_program_brk, current_user_token, exit_current_and_run_next, get_current_task_info,
        mmap, munmap, suspend_current_and_run_next, TaskStatus,
    },
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// Task information
#[allow(dead_code)]
pub struct TaskInfo {
    /// Task status in it's life cycle
    status: TaskStatus,
    /// The numbers of syscall called by task
    syscall_times: [u32; MAX_SYSCALL_NUM],
    /// Total running time of task
    time: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    //获取的就是多个切片的引用
    //获取的是物理地址的引用
    let buffers =
        translated_byte_buffer(current_user_token(), ts as *const u8, size_of::<TimeVal>());
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let mut time_val_ptr = &time_val as *const _ as *const u8;
    for buffer in buffers {
        unsafe {
            time_val_ptr.copy_to(buffer.as_mut_ptr(), buffer.len());
            time_val_ptr = time_val_ptr.add(buffer.len());
        }
    }
    0
}

/// YOUR JOB: Finish sys_task_info to pass testcases
pub fn sys_task_info(ti: *mut TaskInfo) -> isize {
    trace!("kernel: sys_task_info");
    //获取的就是多个切片的引用
    //获取的是物理地址的引用
    let buffers =
        translated_byte_buffer(current_user_token(), ti as *const u8, size_of::<TaskInfo>());
    let task_info = get_current_task_info();
    // trace!("task_info {:?}", task_info.2);
    // println!("task_info 0 {}", task_info.0.);
    // println!("task_info 1 {:?}", task_info.1);
    // println!("task_info 2 {:?}", task_info.2);
    // let mut status_ptr = &task_info.0 as *const _ as *const u8;
    // let mut syscall_times_ptr = &task_info.1 as *const _ as *const u8;
    // let mut time_ptr = &task_info.2 as *const _ as *const u8;
    let task_info = TaskInfo {
        status: task_info.0,
        syscall_times: task_info.1,
        time: task_info.2,
    };
    let mut task_info_ptr = &task_info as *const _ as *const u8;
    for buffer in buffers {
        unsafe {
            task_info_ptr.copy_to(buffer.as_mut_ptr(), buffer.len());
            task_info_ptr = task_info_ptr.add(buffer.len());
        }
    }
    // unsafe {
    //     *ti = TaskInfo {
    //         status: task_info.0,
    //         syscall_times: task_info.1,
    //         time: task_info.2,
    //     };
    // }
    // unsafe {
    //     // Write the status
    //     write(&mut (*ti).status as *mut _, task_info.0);

    //     // Write the syscall_times
    //     write(&mut (*ti).syscall_times as *mut _, task_info.1);

    //     // Write the time
    //     write(&mut (*ti).time as *mut _, task_info.2);
    // }
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    mmap(start, len, port)
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    munmap(start, len)
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
