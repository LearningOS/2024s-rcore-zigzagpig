//! Loading user applications into memory

/// Get the total number of applications.
pub fn get_num_app() -> usize {
    extern "C" {
        fn _num_app();
    }
    unsafe { (_num_app as usize as *const usize).read_volatile() }
}

/// get applications data
pub fn get_app_data(app_id: usize) -> &'static [u8] {
    extern "C" {
        fn _num_app();
    }
    // .quad 11
    // .quad app_0_start
    // .quad app_1_start
    // .quad app_2_start
    // .quad app_3_start
    // .quad app_4_start
    // .quad app_5_start
    // .quad app_6_start
    // .quad app_7_start
    // .quad app_8_start
    // .quad app_9_start
    // .quad app_10_start
    // .quad app_10_end
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app();
    // 获取数组[app_0_start,...,app_10_end] 11个应用长度是12
    // app_start[i] 代表第i个应用开始地址
    // app_start[i + 1] 代表第 i + 1 个应用开始地址
    let app_start = unsafe { core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1) };
    assert!(app_id < num_app);
    unsafe {
        core::slice::from_raw_parts(
            //转成开始地址指针
            //地址都是虚拟地址
            app_start[app_id] as *const u8,
            app_start[app_id + 1] - app_start[app_id],
        )
    }
}
