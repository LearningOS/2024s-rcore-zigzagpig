//! Implementation of [`MapArea`] and [`MemorySet`].

use super::{frame_alloc, FrameTracker};
use super::{PTEFlags, PageTable, PageTableEntry};
use super::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use super::{StepByOne, VPNRange};
use crate::config::{
    KERNEL_STACK_SIZE, MEMORY_END, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE,
};
use crate::sync::UPSafeCell;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use lazy_static::*;
use riscv::register::satp;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// The kernel's initial memory mapping(kernel address space)
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> =
        Arc::new(unsafe { UPSafeCell::new(MemorySet::new_kernel()) });
}
/// address space
pub struct MemorySet {
    page_table: PageTable,
    //各段内存信息存在 Vec 里面
    areas: Vec<MapArea>,
}

impl MemorySet {
    /// Create a new empty `MemorySet`.
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }
    /// Get the page table token
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// Assume that no conflicts.
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }
    /// 根据虚拟地址范围,分配对应的物理页
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        // println!("map_area:{:?}", map_area.vpn_range.get_start());
        // println!("map_area:{:?}", map_area.vpn_range.get_end());
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            // 复制数据到分配的页
            map_area.copy_data(&mut self.page_table, data);
        }
        //处理完后 把段信息放进 vec 里面管理
        self.areas.push(map_area);
    }
    /// Mention that trampoline is not collected by areas.
    /// 最后一页
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            // strampoline 跳板是在编译的时候写好的
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }
    /// Without kernel stacks.
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map kernel sections
        info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        info!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        info!("mapping .text section");
        let text_map_area = MapArea::new(
            (stext as usize).into(),
            (etext as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::X,
        );
        // println!("text_map_area={:?}", text_map_area.vpn_range.get_start());
        // println!("text_map_area={:?}", text_map_area.vpn_range.get_end());
        // println!("text_map_area={}",text_map_area.vpn_range.get_start());
        // println!("text_map_area={}",text_map_area.vpn_range.get_start());
        memory_set.push(text_map_area, None);
        info!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        );
        info!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        memory_set
    }
    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp_base and entry point.
    /// 从 elf 信息创建地址空间
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map program headers of elf, with U flag
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                // 添加一个段
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }
        // map user stack with U flags
        let max_end_va: VirtAddr = max_end_vpn.into();
        // 用户栈底对应虚拟地址的末尾
        let mut user_stack_bottom: usize = max_end_va.into();
        // guard page 栈底往后移动一格,中间空出保护页
        user_stack_bottom += PAGE_SIZE;
        // 栈底是ELF各段的结束位置   (⊙_⊙)?
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE;
        memory_set.push(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // used in sbrk
        memory_set.push(
            MapArea::new(
                user_stack_top.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // map TrapContext
        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT_BASE.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        // 后面不加跳板是因为,一开始就加了跳板
        (
            memory_set,
            user_stack_top,
            elf.header.pt2.entry_point() as usize,
        )
    }
    /// Change page table by writing satp CSR Register.
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            asm!("sfence.vma");
        }
    }
    /// Translate a virtual page number to a page table entry
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }
    /// shrink the area to new_end
    #[allow(unused)]
    pub fn shrink_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.shrink_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// append the area to new_end
    #[allow(unused)]
    pub fn append_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.append_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    ///
    /// syscall ID：222
    ///
    /// 申请长度为 len 字节的物理内存（不要求实际物理内存位置，可以随便找一块），将其映射到 start 开始的虚存，内存页属性为 port
    /// 参数：
    /// - start 需要映射的虚存起始地址，要求按页对齐
    /// - len 映射字节长度，可以为 0
    /// - port：第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行。其他位无效且必须为 0
    ///
    /// 返回值：执行成功则返回 0，错误返回 -1
    ///
    /// 说明：
    /// 为了简单，目标虚存区间要求按页对齐，len 可直接按页向上取整，不考虑分配失败时的页回收。
    ///
    /// 可能的错误：
    /// - start 没有按页大小对齐
    /// - port & !0x7 != 0 (port 其余位必须为0)
    /// - port & 0x7 = 0 (这样的内存无意义)
    /// - [start, start + len) 中存在已经被映射的页
    /// - 物理内存不足
    pub fn mmap(&mut self, start: usize, len: usize, port: usize) -> isize {
        let len = if len < 4096 { 4096 } else { len };

        let start_va: VirtAddr = start.into();
        let start_vpn: VirtPageNum = start_va.floor();

        let end_vpn: VirtPageNum = VirtAddr::from(start + len).ceil();
        let end_va = end_vpn.into();

        if start_va.page_offset() != 0 {
            //debug!("!!!!!!start 没有按页大小对齐");
            return -1;
        }

        // 非低4位不为0
        if port & !0x7 != 0 {
            //debug!("!!!!!!port 其余位必须为0");
            return -1;
        }
        // 低4位为0
        if port & 0x7 == 0 {
            //debug!("!!!!!!这样的内存无意义");
            return -1;
        }

        // 检查虚拟地址是否合法
        // let vpn_range = VPNRange::new(start_vpn, end_vpn);
        // vpn_range.into_iter().any(|vpn| self.areas.iter().)
        for vpn in start_vpn.0..end_vpn.0 {
            for area in &self.areas {
                if area.data_frames.get(&VirtPageNum(vpn)).is_some() {
                    // debug!(
                    //     "!!!!!![start {}, start + len {}) 中存在已经被映射的页",
                    //     start, len
                    // );
                    // debug!(
                    //     "start_va={:?},start_vpn={:?},end_va={:?},end_vpn={:?}",
                    //     start_va, start_vpn, end_va, end_vpn
                    // );
                    return -1;
                }
            }
        }
        // if self
        //     .areas
        //     .iter()
        //     .any(|area| area.intersects(start_vpn, end_vpn))
        // {
        //     println!(
        //         "!!!!!![start {}, start + len {}) 中存在已经被映射的页",
        //         start, len
        //     );
        //     debug!(
        //         "start_va={:?},start_vpn={:?},end_va={:?},end_vpn={:?}",
        //         start_va, start_vpn, end_va, end_vpn
        //     );
        //     return -1;
        // }

        let mut permission = MapPermission::from_bits((port as u8) << 1).unwrap();
        permission.set(MapPermission::U, true);

        self.insert_framed_area(start_va, end_va, permission);
        // for area in &mut self.areas {
        //     debug!("after mmap {:?}", area.data_frames);
        // }
        0
    }

    /// syscall ID：215
    ///
    ///取消到 [start, start + len) 虚存的映射
    ///
    ///参数和返回值请参考 mmap
    ///
    ///说明：
    ///为了简单，参数错误时不考虑内存的恢复和回收。
    ///
    ///可能的错误：
    ///[start, start + len) 中存在未被映射的虚存。
    ///
    ///tips:
    ///
    ///一定要注意 mmap 是的页表项，注意 riscv 页表项的格式与 port 的区别。
    ///
    ///你增加 PTE_U 了吗？
    pub fn munmap(&mut self, start: usize, len: usize) -> isize {
        // let start_va: VirtAddr = start.into();
        // let end_va: VirtAddr = (start + len).into();
        // if start_va.page_offset() != 0 {
        //     println!("start 没有按页大小对齐");
        //     return -1;
        // }
        let len = if len < 4096 { 4096 } else { len };
        let start_va: VirtAddr = start.into();
        let start_vpn: VirtPageNum = start_va.floor();

        let end_vpn: VirtPageNum = VirtAddr::from(start + len).ceil();
        // debug!(
        //     "start_va={:?},start_vpn={:?},--={:?},end_vpn={:?}",
        //     start_va, start_vpn, 0, end_vpn
        // );
        // let end_va: VirtAddr = end_vpn.into();

        // for vpn in start_vpn.0..end_vpn.0 {
        //     let mut flag = false;
        //     for area in self.areas.iter() {
        //         if area.vpn_range.contains(VirtPageNum(vpn)) {
        //             flag = true;
        //             break;
        //         }
        //     }
        //     if !flag {
        //         println!("!!!!!![start, start + len) 中存在未被映射的虚存。");
        //         return -1;
        //     }
        // }
        for vpn in start_vpn.0..end_vpn.0 {
            let mut unmap_success = false;
            for area in &mut self.areas {
                // debug!("{:?}", area.data_frames);
                if area.data_frames.get(&VirtPageNum(vpn)).is_some() {
                    unmap_success = true;
                    area.unmap_one(&mut self.page_table, VirtPageNum(vpn));
                    break;
                }
            }
            if !unmap_success {
                // debug!(
                //     "start_va={:?},start_vpn={:?},vpn={:?},end_vpn={:?}",
                //     start_va, start_vpn, vpn, end_vpn
                // );
                // debug!("!!!!!![start, start + len) 中存在未被映射的虚存。");
                return -1;
            }
        }

        // // 检查虚拟地址是否合法
        // if self
        //     .areas
        //     .iter()
        //     .any(|area| area.intersects(start_va.floor(), end_va.ceil()))
        // {
        //     println!("[start, start + len) 中存在已经被映射的页");
        //     return -1;
        // }

        // let mut permission = MapPermission::from_bits((port as u8) << 1).unwrap();
        // permission.set(MapPermission::U, true);

        // self.insert_framed_area(start_va, (start + len).into(), permission);
        0
    }
}
/// map area structure, controls a contiguous piece of virtual memory
/// MapArea 翻译成段比较好
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

impl MapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();

        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
    // pub fn intersects(&self, start: VirtPageNum, end: VirtPageNum) -> bool {
    //     self.vpn_range.intersects(&VPNRange::new(start, end))
    // }
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        // println!("MapType={:?}", self.map_type);
        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            // 分配物理页号,并与虚拟页号匹配
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        // println!("ppn={}", ppn.0);
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
        // println!("after map");
    }
    #[allow(unused)]
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        // 释放物理页
        if self.map_type == MapType::Framed {
            self.data_frames.remove(&vpn);
        }
        // 数据物理页存放数据,页表物理页存放页表条目
        page_table.unmap(vpn);
    }
    /// 将页表的所有虚拟页号,分配物理页号,并匹配
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            // debug!("vpn:{}", vpn.0);
            self.map_one(page_table, vpn);
        }
    }
    #[allow(unused)]
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }
    // end 缩小到 new_end ,释放减少的部分. 更改段内存范围
    #[allow(unused)]
    pub fn shrink_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(new_end, self.vpn_range.get_end()) {
            self.unmap_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    #[allow(unused)]
    pub fn append_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(self.vpn_range.get_end(), new_end) {
            self.map_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    /// data: start-aligned but maybe with shorter length
    /// assume that all frames were cleared before
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
/// map type for memory set: identical or framed
pub enum MapType {
    Identical,
    Framed,
}

bitflags! {
    /// map permission corresponding to that in pte: `R W X U`
    pub struct MapPermission: u8 {
        ///Readable
        const R = 1 << 1;
        ///Writable
        const W = 1 << 2;
        ///Excutable
        const X = 1 << 3;
        ///Accessible in U mode
        const U = 1 << 4;
    }
}

/// Return (bottom, top) of a kernel stack in kernel space.
/// 每个应用给两页空间 KERNEL_STACK_SIZE = 8192
/// 这里是跳板
///    top: fffffffffffff000
/// bottom: ffffffffffffd000 应用 0  d000 - f000
/// top: ffffffffffffc000    中间 c000 是空的
/// bottom: ffffffffffffa000 应用 1  a000 - b000  
/// top: ffffffffffff9000
/// bottom: ffffffffffff7000   
/// top: ffffffffffff6000
/// bottom: ffffffffffff4000   
/// top: ffffffffffff3000
/// bottom: ffffffffffff1000   
/// top: ffffffffffff0000
/// bottom: fffffffffffee000   
/// top: fffffffffffed000
/// bottom: fffffffffffeb000   
/// top: fffffffffffea000
/// bottom: fffffffffffe8000   
/// top: fffffffffffe7000
/// bottom: fffffffffffe5000   
/// top: fffffffffffe4000
/// bottom: fffffffffffe2000   
/// top: fffffffffffe1000
/// bottom: fffffffffffdf000   
/// top: fffffffffffde000
/// bottom: fffffffffffdc000   
/// top: fffffffffffdb000
/// bottom: fffffffffffd9000   
/// top: fffffffffffd8000
/// bottom: fffffffffffd6000   
/// top: fffffffffffd5000
/// bottom: fffffffffffd3000   
/// top: fffffffffffd2000
/// bottom: fffffffffffd0000  
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

/// remap test in kernel space
#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.exclusive_access();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert!(!kernel_space
        .page_table
        .translate(mid_text.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_rodata.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_data.floor())
        .unwrap()
        .executable(),);
    println!("remap_test passed!");
}
