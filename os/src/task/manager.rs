//!Implementation of [`TaskManager`]
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A simple FIFO scheduler.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        debug!("before: there are {} tasks", self.ready_queue.len());
        self.ready_queue.push_back(task);
        debug!("after: there are {} tasks", self.ready_queue.len());
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // debug!("in fetch_task");
        // let len = self.ready_queue.len();
        // debug!("len:{len}");
        // if len == 0 {
        //     panic!("in fetch_task len == 0");
        // }
        // static mut COUNT: usize = 1usize;
        // unsafe {
        //     if COUNT == 2 {
        //         panic!("count==2");
        //     }
        // }
        self.ready_queue.pop_front()
    }

    /// Take a process out of the ready queue
    pub fn fetch_min_task_stride(&mut self) -> Option<Arc<TaskControlBlock>> {
        // let len = self.ready_queue.len();
        let mut index = 0;
        // if len == 0 {
        //     // panic!("fetch_min_task_stride, len == 0");
        // }
        // let mut min_stride_task = None;
        // debug!("fetch_min_task_stride in : {len} tasks");

        for (i, tcb) in self.ready_queue.iter_mut().enumerate() {
            if index >= tcb.inner_exclusive_access().task_stride {
                index = i;
            }
        }
        self.ready_queue.remove(index)
        // for i in 0..len {
        //     // debug!("ready_queue is {:?}", self.ready_queue[i].pid.0);
        //     // debug!("ready_queue[{i}]:",);
        //     if let Some(current) = self.ready_queue.pop_front() {
        //         // debug!("current    pid:   {:?}", current.pid.0);
        //         // debug!(
        //         //     "current   task_stride    is:   {:?}",
        //         //     current.inner_exclusive_access().task_stride
        //         // );
        //         // debug!("----------");
        //         // min_stride = min_stride.min(current.inner_exclusive_access().task_stride);
        //         // let current =current.inner_exclusive_access();
        //         if min_stride >= current.inner_exclusive_access().task_stride {
        //             min_stride = current.inner_exclusive_access().task_stride;
        //             // min_stride_task = Some(Arc::clone(&current));
        //         }
        //         self.ready_queue.push_back(current);
        //     } else {
        //         return None;
        //     }
        // }
        // debug!(
        //     "after for:fetch_min_task_stride remain : {} tasks",
        //     self.ready_queue.len()
        // );
        // for _ in 0..len {
        //     if let Some(current) = self.ready_queue.pop_front() {
        //         // min_stride = min_stride.min(current.inner_exclusive_access().task_stride);
        //         // let current =current.inner_exclusive_access();
        //         if min_stride == current.inner_exclusive_access().task_stride {
        //             debug!("select   pid:  {:?}", current.pid.0);

        //             return Some(current);
        //             // min_stride = current.inner_exclusive_access().task_stride;
        //             // min_stride_task = Some(Arc::clone(&current));
        //             // self.ready_queue.push_back(current);
        //         }
        //         self.ready_queue.push_back(current);
        //     } else {
        //         return None;
        //     }
        // }
        // None
        // min_stride_task
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}

/// Take a process out of the ready queue
pub fn fetch_min_task_stride() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch_min_task_stride()
}
