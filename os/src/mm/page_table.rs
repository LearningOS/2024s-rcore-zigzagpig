//! Implementation of [`PageTableEntry`] and [`PageTable`].

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
/// 翻译成页表条目就顺眼多了
/// 低8位是权限位,8-9位备用,10-53位是物理页号
pub struct PageTableEntry {
    /// bits of page table entry
    pub bits: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    /// Create an empty page table entry
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// Get the physical page number from the page table entry
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /// Get the flags from the page table entry
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /// The page pointered by page table entry is valid?
    /// 当前标志位是合法的
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is readable?
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is writable?
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is executable?
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// page table structure
/// ```
/// pub struct PageTable {
///     root_ppn: PhysPageNum,
///     frames: Vec<FrameTracker>,
/// }
/// ```
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// Create a new page table
    pub fn new() -> Self {
        // 分配了一个页帧,大小就是一个物理页
        // 一页size = 32 字节(0x20),一共4096字节,只能装128个
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    /// 上图是 RISC-V 64 架构下 satp 的字段分布，含义如下：
    /// - 高4位: MODE 控制 CPU 使用哪种页表实现；
    /// - 中:16位: ASID 表示地址空间标识符，这里还没有涉及到进程的概念，我们不需要管这个地方；
    /// - 低44位: PPN 存的是根页表所在的物理页号。这样，给定一个虚拟页号，CPU 就可以从三级页表的根页表开始一步步的将其映射到一个物理页号。
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /// Find PageTableEntry by VirtPageNum, create a frame for a 4KB page table if not exist
    ///
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        // 获取虚拟页号三级页表的三部分
        // 虚拟地址只是用来查表的
        // println!("map vpn {}", vpn.0);
        // println!("PageTable {:?}", self.frames);
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            //获取页表条目的可变引用,获取一个包含页表条目指针的数组
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                //只是一个标记
                self.frames.push(frame);
            }
            // 这个物理地址是新分配的,上面就报证了是合法的
            ppn = pte.ppn();
        }
        result
    }
    /// Find PageTableEntry by VirtPageNum
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    /// set the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        // 查找页表条目
        let pte = self.find_pte_create(vpn).unwrap();
        // println!("vpn:{}--------", vpn.0);
        // 在这个项目中肯定不会发生 !pte.is_valid() ?
        // 然而第一个出现问题就是在这里!!!
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        // 让查到的页表条目,它的值改成要绑定的物理页数
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    /// remove the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    /// get the page table entry from the virtual page number
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| *pte)
    }
    /// get the token from the page table
    /// satp高4位设置为8,低44位是物理页号
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

/// Translate&Copy a ptr[u8] array with LENGTH len to a mutable u8 Vec through page table
/// 这样翻译真的行吗?satp都没换直接获取引用
/// 直接获取每个字节的引用,然后操作
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    //根据satp创建页表,获取页表
    let page_table = PageTable::from_token(token);
    //指针值就是个虚拟地址值
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        //指针转成虚拟地址
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        //由虚拟地址查找对应页表项(条目),进而获得物理页数
        let ppn = page_table.translate(vpn).unwrap().ppn();
        //获取下一个虚拟页数
        vpn.step();
        //下一页对应虚拟地址,对应就是下一虚拟页数偏移量为0的位置
        let mut end_va: VirtAddr = vpn.into();
        //结束地址和本页结束位置取小值,逐页处理,处理不完的下次循环处理
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            //页偏移地址为0,说明要从开始位置读完本页
            //先获取整页,然后取需要的部分
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            // 否则读到结束位置就获取了所有数据
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        //继续处理下一页
        start = end_va.into();
    }
    v
}
