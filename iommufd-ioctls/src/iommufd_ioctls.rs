// Copyright © 2025 Crusoe Energy Systems LLC
//
// SPDX-License-Identifier: Apache-2.0
//

use std::fs::{self, File, OpenOptions};
use std::mem::ManuallyDrop;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;
use std::sync::Arc;

use iommufd_bindings::iommufd::*;
use vmm_sys_util::errno::Error as SysError;

use crate::{IommufdError, Result};

// vIOMMU type constants (from linux/iommufd.h)
// These should match the kernel's enum iommu_viommu_type
pub const IOMMU_VIOMMU_TYPE_DEFAULT: u32 = 0;
pub const IOMMU_VIOMMU_TYPE_ARM_SMMUV3: u32 = 1;
pub const IOMMU_VIOMMU_TYPE_TEGRA241_CMDQV: u32 = 2;

/// Detect if the system has NVIDIA CMDQV hardware (GH200/GB200)
/// by checking for ACPI devices with HID "NVDA200C"
fn detect_cmdqv_hardware() -> bool {
    // Check /sys/bus/acpi/devices for NVDA200C devices
    let acpi_path = Path::new("/sys/bus/acpi/devices");
    if let Ok(entries) = fs::read_dir(acpi_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("NVDA200C") {
                eprintln!("IOMMUFD: Detected NVIDIA CMDQV hardware: {:?}", name);
                return true;
            }
        }
    }

    // Alternative: check /sys/devices for platform devices with CMDQV
    let platform_path = Path::new("/sys/devices/platform");
    if let Ok(entries) = fs::read_dir(platform_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().contains("cmdqv") {
                eprintln!("IOMMUFD: Detected CMDQV platform device: {:?}", name);
                return true;
            }
        }
    }

    false
}

/// Get the appropriate vIOMMU type for the current hardware
pub fn get_viommu_type() -> u32 {
    if detect_cmdqv_hardware() {
        IOMMU_VIOMMU_TYPE_TEGRA241_CMDQV
    } else {
        IOMMU_VIOMMU_TYPE_ARM_SMMUV3
    }
}

pub struct IommuFd {
    iommufd: File,
}

impl IommuFd {
    pub fn new() -> Result<Self> {
        let iommufd = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/iommu")
            .map_err(IommufdError::OpenIommufd)?;

        Ok(IommuFd { iommufd })
    }

    /// Wrap an already-open iommufd file (e.g. one passed in from a
    /// higher layer). Kept API-compatible with the upstream crate.
    pub fn new_from_fd(file: File) -> Self {
        IommuFd { iommufd: file }
    }

    pub fn destroy_iommu_object(&self, id: u32) -> Result<()> {
        let destroy_data = iommu_destroy {
            size: std::mem::size_of::<iommu_destroy>() as u32,
            id,
        };
        iommufd_syscall::destroy_iommufd(self, &destroy_data)
    }

    pub fn alloc_iommu_ioas(&self, alloc_data: &mut iommu_ioas_alloc) -> Result<()> {
        iommufd_syscall::alloc_iommu_ioas(self, alloc_data)
    }

    pub fn map_iommu_ioas(&self, map: &iommu_ioas_map) -> Result<()> {
        iommufd_syscall::map_iommu_ioas(self, map)
    }

    pub fn unmap_iommu_ioas(&self, unmap: &mut iommu_ioas_unmap) -> Result<()> {
        iommufd_syscall::unmap_iommu_ioas(self, unmap)
    }

    /// Map a file-backed range into the IOAS.
    ///
    /// `IOMMU_IOAS_MAP_FILE` takes the same flags and semantics as
    /// `IOMMU_IOAS_MAP` but sources the pages from `map.fd` at byte offset
    /// `map.start`. Callers set `IOMMU_IOAS_MAP_FIXED_IOVA`, which makes
    /// `iova` an input, so the kernel writes no field back and the argument
    /// is taken by shared reference like `map_iommu_ioas`.
    pub fn map_file_iommu_ioas(&self, map: &iommu_ioas_map_file) -> Result<()> {
        iommufd_syscall::map_file_iommu_ioas(self, map)
    }

    /// Query the IOAS for its allowed IOVA ranges and minimum IOVA
    /// alignment.
    ///
    /// A caller that passes `num_iovas = 0` gets back `EMSGSIZE` with
    /// `out_iova_alignment` filled in regardless (the kernel only needs
    /// the range list itself, not the alignment, to fit the caller's
    /// buffer). This verb stays faithful to the ioctl and returns `Err`
    /// for any `ret < 0`, `EMSGSIZE` included; tolerating that errno to
    /// read `out_iova_alignment` anyway is the caller's job.
    pub fn iova_ranges_iommu_ioas(&self, ranges: &mut iommu_ioas_iova_ranges) -> Result<()> {
        iommufd_syscall::iova_ranges_iommu_ioas(self, ranges)
    }

    pub fn alloc_iommu_hwpt(&self, hwpt_alloc: &mut iommu_hwpt_alloc) -> Result<()> {
        iommufd_syscall::alloc_iommu_hwpt(self, hwpt_alloc)
    }

    pub fn get_hw_info(&self, hw_info: &mut iommu_hw_info) -> Result<()> {
        iommufd_syscall::get_hw_info(self, hw_info)
    }

    pub fn invalidate_hwpt(&self, hwpt_invalidate: &mut iommu_hwpt_invalidate) -> Result<()> {
        iommufd_syscall::invalidate_hwpt(self, hwpt_invalidate)
    }

    pub fn alloc_iommu_viommu(&self, viommu_alloc: &mut iommu_viommu_alloc) -> Result<()> {
        iommufd_syscall::alloc_iommu_viommu(self, viommu_alloc)
    }

    pub fn alloc_iommu_vdevice(&self, vdevice_alloc: &mut iommu_vdevice_alloc) -> Result<()> {
        iommufd_syscall::alloc_iommu_vdevice(self, vdevice_alloc)
    }

    pub fn alloc_iommu_veventq(&self, veventq_alloc: &mut iommu_veventq_alloc) -> Result<()> {
        iommufd_syscall::alloc_iommu_veventq(self, veventq_alloc)
    }

    pub fn alloc_iommu_hw_queue(&self, hw_queue_alloc: &mut iommu_hw_queue_alloc) -> Result<()> {
        iommufd_syscall::alloc_hw_queue(self, hw_queue_alloc)
    }
}

#[derive(Debug, Clone)]
pub enum IommufdInvalidateData {
    Smmuv3(iommu_viommu_arm_smmuv3_invalidate),
    Vtd(iommu_hwpt_vtd_s1_invalidate),
    /// Driver-specific invalidation entries in raw form: `data_type` is a
    /// value of `enum iommu_hwpt_invalidate_data_type`, `entry_len` the
    /// size of one entry, and `entries` a packed array of entries.
    Raw {
        data_type: u32,
        entry_len: u32,
        entries: Vec<u8>,
    },
}

#[derive(Clone)]
pub struct IommufdVIommu {
    pub iommufd: Arc<IommuFd>,
    pub viommu_id: u32,
    pub dev_id: u32,
    pub s2_hwpt_id: u32,
    pub bypass_hwpt_id: Option<u32>,
    pub abort_hwpt_id: Option<u32>,
    /// Driver data returned by the kernel on a Tegra241 CMDQV vIOMMU
    /// allocation (e.g. the VINTF page0 mmap coordinates). `None` for
    /// non-CMDQV vIOMMU types.
    pub cmdqv_data: Option<iommu_viommu_tegra241_cmdqv>,
}

impl IommufdVIommu {
    /// Create a new vIOMMU instance
    /// # Arguments
    /// * `iommufd` - The iommufd instance to use
    /// * `ioas_id` - The IOAS ID to associate with the vIOMMU
    /// * `dev_id` - The device ID of the VFIO device
    /// * `s1_hwpt_data_type` - The s1 hwpt data type
    pub fn new(
        iommufd: Arc<IommuFd>,
        ioas_id: u32,
        dev_id: u32,
        s1_hwpt_data_type: iommu_hwpt_data_type,
    ) -> Result<Self> {
        if s1_hwpt_data_type != iommu_hwpt_data_type_IOMMU_HWPT_DATA_ARM_SMMUV3 {
            return Err(IommufdError::UnsupportedS1HwptDataType(s1_hwpt_data_type));
        }

        // Refer to “5.2 Stream Table Entry” in SMMUv3 HW Specification
        const SMMU_STE_VALID: u64 = 1 << 0;
        const SMMU_STE_CFG_BYPASS: u64 = 1 << 3;

        // Use the auto-detected type for CMDQV support. For CMDQV the
        // kernel returns the VINTF page0 mmap coordinates as driver data.
        let viommu_type = get_viommu_type();
        let mut cmdqv_data = iommu_viommu_tegra241_cmdqv::default();
        let viommu_data = if viommu_type == IOMMU_VIOMMU_TYPE_TEGRA241_CMDQV {
            Some((
                &mut cmdqv_data as *mut iommu_viommu_tegra241_cmdqv as u64,
                std::mem::size_of::<iommu_viommu_tegra241_cmdqv>() as u32,
            ))
        } else {
            None
        };

        let mut viommu = Self::new_with_type(iommufd, ioas_id, dev_id, viommu_type, viommu_data)?;

        // Allocate the bypass s1_hwpt used while the virtual IOMMU is not
        // yet initialized by the guest, and the abort s1_hwpt used when
        // the guest configures a stream for abort.
        // The parent must be the vIOMMU, not the S2 nesting-parent HWPT:
        // mainline arm-smmu-v3 only implements nested-domain allocation
        // as a vIOMMU op (hw_pagetable.c rejects an hwpt-parented nested
        // alloc with EOPNOTSUPP when the driver lacks the device-level
        // op). This also matches QEMU's allocation order.
        let s1_pt_id = viommu.viommu_id;

        let bypass_s1_hwpt_data = iommu_hwpt_arm_smmuv3 {
            ste: [SMMU_STE_CFG_BYPASS | SMMU_STE_VALID, 0x0],
        };
        let mut bypass_iommufd_hwpt_alloc = iommu_hwpt_alloc {
            size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
            dev_id,
            pt_id: s1_pt_id,
            data_type: s1_hwpt_data_type,
            data_len: std::mem::size_of::<iommu_hwpt_arm_smmuv3>() as u32,
            data_uptr: &bypass_s1_hwpt_data as *const iommu_hwpt_arm_smmuv3 as u64,
            ..Default::default()
        };
        viommu
            .iommufd
            .alloc_iommu_hwpt(&mut bypass_iommufd_hwpt_alloc)?;
        viommu.bypass_hwpt_id = Some(bypass_iommufd_hwpt_alloc.out_hwpt_id);

        let abort_s1_hwpt_data = iommu_hwpt_arm_smmuv3 {
            ste: [SMMU_STE_VALID, 0x0],
        };
        let mut abort_iommufd_hwpt_alloc = iommu_hwpt_alloc {
            size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
            dev_id,
            pt_id: s1_pt_id,
            data_type: s1_hwpt_data_type,
            data_len: std::mem::size_of::<iommu_hwpt_arm_smmuv3>() as u32,
            data_uptr: &abort_s1_hwpt_data as *const iommu_hwpt_arm_smmuv3 as u64,
            ..Default::default()
        };
        viommu
            .iommufd
            .alloc_iommu_hwpt(&mut abort_iommufd_hwpt_alloc)?;
        viommu.abort_hwpt_id = Some(abort_iommufd_hwpt_alloc.out_hwpt_id);

        if viommu_type == IOMMU_VIOMMU_TYPE_TEGRA241_CMDQV {
            viommu.cmdqv_data = Some(cmdqv_data);
        }

        Ok(viommu)
    }

    /// Create a vIOMMU of an explicit type, without any driver-specific
    /// child HWPTs.
    ///
    /// Allocates the stage-2 nesting-parent HWPT over `ioas_id` for
    /// `dev_id` and a vIOMMU of `viommu_type` on top of it. Driver data
    /// for the vIOMMU allocation can be passed as a raw
    /// (user pointer, length) pair in `viommu_data`.
    ///
    /// # Arguments
    /// * `iommufd` - The iommufd instance to use
    /// * `ioas_id` - The IOAS the stage-2 nesting parent translates
    /// * `dev_id` - The iommufd device ID the allocations validate against
    /// * `viommu_type` - A value of `enum iommu_viommu_type`
    /// * `viommu_data` - Optional (data_uptr, data_len) driver data
    pub fn new_with_type(
        iommufd: Arc<IommuFd>,
        ioas_id: u32,
        dev_id: u32,
        viommu_type: u32,
        viommu_data: Option<(u64, u32)>,
    ) -> Result<Self> {
        // Allocate the s2_hwpt shared by all devices behind this vIOMMU
        // instance.
        let mut s2_iommufd_hwpt_alloc = iommu_hwpt_alloc {
            size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
            flags: iommufd_hwpt_alloc_flags_IOMMU_HWPT_ALLOC_NEST_PARENT,
            dev_id,
            pt_id: ioas_id,
            data_type: iommu_hwpt_data_type_IOMMU_HWPT_DATA_NONE,
            ..Default::default()
        };
        iommufd.alloc_iommu_hwpt(&mut s2_iommufd_hwpt_alloc)?;
        let s2_hwpt_id = s2_iommufd_hwpt_alloc.out_hwpt_id;

        let (data_uptr, data_len) = viommu_data.unwrap_or((0, 0));
        let mut viommu_alloc = iommu_viommu_alloc {
            size: std::mem::size_of::<iommu_viommu_alloc>() as u32,
            type_: viommu_type,
            hwpt_id: s2_hwpt_id,
            dev_id,
            data_len,
            data_uptr,
            ..Default::default()
        };
        iommufd.alloc_iommu_viommu(&mut viommu_alloc)?;

        Ok(IommufdVIommu {
            iommufd,
            viommu_id: viommu_alloc.out_viommu_id,
            dev_id,
            s2_hwpt_id,
            bypass_hwpt_id: None,
            abort_hwpt_id: None,
            cmdqv_data: None,
        })
    }

    /// The VINTF page0 mmap coordinates `(offset, length)` for a
    /// Tegra241 CMDQV vIOMMU, as returned by the kernel on allocation.
    /// The offset is a BYTE offset to pass directly as mmap()'s offset
    /// argument, not a page number.
    ///
    /// Returns `None` if this vIOMMU is not a CMDQV vIOMMU, AND also if
    /// it is a CMDQV vIOMMU that was constructed directly via
    /// `new_with_type()` rather than `new()` - `cmdqv_data` is only
    /// populated by `new()`.
    pub fn vintf_page0(&self) -> Option<(u64, u64)> {
        self.cmdqv_data
            .map(|d| (d.out_vintf_page0_pgoff, d.out_vintf_page0_pgsz))
    }

    /// Allocate a virtual event queue for this vIOMMU.
    ///
    /// # Arguments
    /// * `veventq_type` - A value of `enum iommu_veventq_type`
    /// * `depth` - Maximum number of events held in the vEVENTQ
    pub fn alloc_veventq(&self, veventq_type: u32, depth: u32) -> Result<IommufdVEventQ> {
        let mut veventq_alloc = iommu_veventq_alloc {
            size: std::mem::size_of::<iommu_veventq_alloc>() as u32,
            viommu_id: self.viommu_id,
            type_: veventq_type,
            veventq_depth: depth,
            ..Default::default()
        };
        self.iommufd.alloc_iommu_veventq(&mut veventq_alloc)?;

        // SAFETY: on success the kernel returns a fresh, open fd that we
        // exclusively own from this point on.
        let fd = unsafe { File::from_raw_fd(veventq_alloc.out_veventq_fd as RawFd) };

        Ok(IommufdVEventQ {
            iommufd: self.iommufd.clone(),
            veventq_id: veventq_alloc.out_veventq_id,
            fd: ManuallyDrop::new(fd),
        })
    }

    /// Allocate a HW queue for this vIOMMU, for a vIOMMU-specific
    /// HW-accelerated queue (e.g. a Tegra241 CMDQV VCMDQ) whose memory is
    /// addressed by `nesting_parent_iova`/`length` in the guest physical
    /// address space.
    ///
    /// # Arguments
    /// * `queue_type` - A value of `enum iommu_hw_queue_type`
    /// * `index` - The logical index of the HW queue per vIOMMU, per type
    /// * `nesting_parent_iova` - Base address of the queue memory
    /// * `length` - Length of the queue memory
    pub fn alloc_hw_queue(
        &self,
        queue_type: u32,
        index: u32,
        nesting_parent_iova: u64,
        length: u64,
    ) -> Result<IommufdHwQueue> {
        let mut hw_queue_alloc = iommu_hw_queue_alloc {
            size: std::mem::size_of::<iommu_hw_queue_alloc>() as u32,
            viommu_id: self.viommu_id,
            type_: queue_type,
            index,
            nesting_parent_iova,
            length,
            ..Default::default()
        };
        self.iommufd.alloc_iommu_hw_queue(&mut hw_queue_alloc)?;

        Ok(IommufdHwQueue {
            iommufd: self.iommufd.clone(),
            hw_queue_id: hw_queue_alloc.out_hw_queue_id,
        })
    }

    /// Invalidate a hwpt entry
    /// # Arguments
    /// * `cmd` - The invalidate data
    /// # Returns:
    /// * `Ok(true)` if the entry is invalidated
    /// * `Ok(false)` if the entry is not invalidated
    pub fn invalidate_hwpt(&self, cmd: &mut IommufdInvalidateData) -> Result<bool> {
        match cmd {
            IommufdInvalidateData::Smmuv3(data) => {
                let mut hw_invalidate = iommu_hwpt_invalidate {
                    size: std::mem::size_of::<iommu_hwpt_invalidate>() as u32,
                    hwpt_id: self.viommu_id,
                    data_type:
                        iommu_hwpt_invalidate_data_type_IOMMU_VIOMMU_INVALIDATE_DATA_ARM_SMMUV3,
                    entry_len: std::mem::size_of::<iommu_viommu_arm_smmuv3_invalidate>() as u32,
                    entry_num: 1,
                    data_uptr: data as *mut iommu_viommu_arm_smmuv3_invalidate as u64,
                    ..Default::default()
                };
                let result = self.iommufd.invalidate_hwpt(&mut hw_invalidate);
                result?;

                if hw_invalidate.entry_num == 1 {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            IommufdInvalidateData::Vtd(_) => {
                unimplemented!()
            }
            IommufdInvalidateData::Raw {
                data_type,
                entry_len,
                entries,
            } => {
                if *entry_len == 0 || entries.len() % (*entry_len as usize) != 0 {
                    return Err(IommufdError::InvalidInvalidateData);
                }
                let entry_num = (entries.len() / (*entry_len as usize)) as u32;
                let mut hw_invalidate = iommu_hwpt_invalidate {
                    size: std::mem::size_of::<iommu_hwpt_invalidate>() as u32,
                    hwpt_id: self.viommu_id,
                    data_type: *data_type,
                    entry_len: *entry_len,
                    entry_num,
                    data_uptr: entries.as_ptr() as u64,
                    ..Default::default()
                };
                self.iommufd.invalidate_hwpt(&mut hw_invalidate)?;
                Ok(hw_invalidate.entry_num == entry_num)
            }
        }
    }
}

impl Drop for IommufdVIommu {
    fn drop(&mut self) {
        // Destroy in reverse order of creation - children before parents
        // Order: bypass_hwpt -> abort_hwpt -> viommu -> s2_hwpt

        if let Some(bypass_hwpt_id) = self.bypass_hwpt_id
            && let Err(e) = self.iommufd.destroy_iommu_object(bypass_hwpt_id)
        {
            eprintln!("Failed to destroy bypass_hwpt id {bypass_hwpt_id}: {e}");
        }

        if let Some(abort_hwpt_id) = self.abort_hwpt_id
            && let Err(e) = self.iommufd.destroy_iommu_object(abort_hwpt_id)
        {
            eprintln!("Failed to destroy abort_hwpt id {abort_hwpt_id}: {e}");
        }

        if let Err(e) = self.iommufd.destroy_iommu_object(self.viommu_id) {
            eprintln!("Failed to destroy vIOMMU id {}: {}", self.viommu_id, e);
        }

        if let Err(e) = self.iommufd.destroy_iommu_object(self.s2_hwpt_id) {
            eprintln!("Failed to destroy s2_hwpt id {}: {}", self.s2_hwpt_id, e);
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum IommufdHwInfoData {
    Smmuv3(iommu_hw_info_arm_smmuv3),
    Vtd(iommu_hw_info_vtd),
    Tegra241Cmdqv(iommu_hw_info_tegra241_cmdqv),
}

#[derive(Debug, Clone)]
pub enum IommufdHwptData {
    Smmuv3(iommu_hwpt_arm_smmuv3),
    Vtd(iommu_hwpt_vtd_s1),
    /// Driver-specific HWPT data in raw form: `data_type` is a value of
    /// `enum iommu_hwpt_data_type` and `data` the packed driver struct.
    Raw {
        data_type: u32,
        data: Vec<u8>,
    },
}

/// A virtual event queue tied to a vIOMMU. Events are read from `fd`;
/// the queue object is destroyed on drop.
///
/// `fd` is `ManuallyDrop` (transparent `Deref`/`DerefMut` to `File`,
/// so callers read/`try_clone()` it exactly as before) because
/// `iommufd_eventq_init` (kernel `drivers/iommu/iommufd/eventq.c`)
/// makes the struct file backing it hold an extra reference on the
/// vEVENTQ object: `IOMMU_DESTROY` always fails `EBUSY`
/// (`refcount_dec_if_one`) while ANY fd on that struct file - this
/// one included - is still open, even one about to be dropped in the
/// same function. `Drop` below closes `fd` first for exactly that
/// reason. This does NOT cover clones: a caller that duplicated `fd`
/// (e.g. `fd.try_clone()`) must close every clone before dropping (or
/// otherwise triggering the destroy of) this object, or the destroy
/// ioctl still fails EBUSY and leaks the vEVENTQ - and, since it
/// holds a reference to its parent, the vIOMMU too - until the whole
/// iommufd context closes.
pub struct IommufdVEventQ {
    pub iommufd: Arc<IommuFd>,
    pub veventq_id: u32,
    pub fd: ManuallyDrop<File>,
}

impl Drop for IommufdVEventQ {
    fn drop(&mut self) {
        // Close our own fd BEFORE asking the kernel to destroy the
        // object - see the struct's doc comment for why this order
        // is required (EBUSY otherwise).
        //
        // SAFETY: `self.fd` is a `ManuallyDrop` precisely so normal
        // drop glue never touches it; this `take` is the only one
        // that ever runs for a given `IommufdVEventQ` (Drop::drop
        // runs at most once), and nothing reads `self.fd` afterward.
        let fd = unsafe { ManuallyDrop::take(&mut self.fd) };
        drop(fd);

        if let Err(e) = self.iommufd.destroy_iommu_object(self.veventq_id) {
            eprintln!("Failed to destroy vEVENTQ id {}: {}", self.veventq_id, e);
        }
    }
}

/// A vIOMMU-specific HW-accelerated queue (e.g. a Tegra241 CMDQV VCMDQ),
/// backed by guest queue memory. The HW queue object is destroyed on drop.
pub struct IommufdHwQueue {
    pub iommufd: Arc<IommuFd>,
    pub hw_queue_id: u32,
}

impl Drop for IommufdHwQueue {
    fn drop(&mut self) {
        if let Err(e) = self.iommufd.destroy_iommu_object(self.hw_queue_id) {
            eprintln!("Failed to destroy HW queue id {}: {}", self.hw_queue_id, e);
        }
    }
}

#[derive(Clone)]
pub struct IommufdVDevice {
    pub viommu: Arc<IommufdVIommu>,
    pub dev_id: u32,
    pub virt_id: u64,
    pub vdevice_id: u32,
    /// The installed stage-1 HWPT: the one the caller has attached the
    /// device to (or is about to attach it to, on a fresh install).
    pub s1_hwpt_id: Option<u32>,
    /// A second stage-1 HWPT allocated by `stage_s1_hwpt` while
    /// `s1_hwpt_id` stays installed, so a caller can replace a live
    /// stage-1 in one attach instead of parking the device on another
    /// HWPT first. Promoted by `commit_staged_s1_hwpt`, released by
    /// `discard_staged_s1_hwpt`; at most one may be pending.
    pub staged_s1_hwpt_id: Option<u32>,
}

impl IommufdVDevice {
    /// Create a new vDevice instance
    /// # Arguments
    /// * `viommu` - The vIOMMU instance the vDevice is associated with
    /// * `dev_id` - The device ID of the vDevice
    /// * `virt_id` - The virtual Stream ID of the vDevice
    pub fn new(viommu: Arc<IommufdVIommu>, dev_id: u32, virt_id: u64) -> Result<Self> {
        let mut vdevice_alloc = iommu_vdevice_alloc {
            size: std::mem::size_of::<iommu_vdevice_alloc>() as u32,
            viommu_id: viommu.viommu_id,
            dev_id,
            virt_id,
            ..Default::default()
        };
        viommu.iommufd.alloc_iommu_vdevice(&mut vdevice_alloc)?;

        Ok(IommufdVDevice {
            viommu,
            dev_id,
            virt_id,
            vdevice_id: vdevice_alloc.out_vdevice_id,
            s1_hwpt_id: None,
            staged_s1_hwpt_id: None,
        })
    }

    /// Issue `IOMMU_HWPT_ALLOC` for a nested stage-1 HWPT parented on the
    /// vIOMMU and return the new object id. Touches no bookkeeping: the
    /// caller decides whether the id becomes the installed or the staged
    /// stage-1.
    fn alloc_s1_hwpt_object(&self, hwpt_data: &IommufdHwptData) -> Result<u32> {
        let mut s1_iommufd_hwpt_alloc = match hwpt_data {
            IommufdHwptData::Smmuv3(data) => iommu_hwpt_alloc {
                size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
                dev_id: self.dev_id,
                pt_id: self.viommu.viommu_id,
                data_type: iommu_hwpt_data_type_IOMMU_HWPT_DATA_ARM_SMMUV3,
                data_len: std::mem::size_of::<iommu_hwpt_arm_smmuv3>() as u32,
                data_uptr: data as *const iommu_hwpt_arm_smmuv3 as u64,
                ..Default::default()
            },
            IommufdHwptData::Vtd(_) => unimplemented!(),
            IommufdHwptData::Raw { data_type, data } => iommu_hwpt_alloc {
                size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
                dev_id: self.dev_id,
                pt_id: self.viommu.viommu_id,
                data_type: *data_type,
                data_len: data.len() as u32,
                data_uptr: data.as_ptr() as u64,
                ..Default::default()
            },
        };
        self.viommu
            .iommufd
            .alloc_iommu_hwpt(&mut s1_iommufd_hwpt_alloc)?;
        Ok(s1_iommufd_hwpt_alloc.out_hwpt_id)
    }

    /// Allocate the stage-1 HWPT for a vDevice that has none installed.
    ///
    /// Equivalent to `stage_s1_hwpt` followed by `commit_staged_s1_hwpt`
    /// with nothing to destroy; fails with `S1HwptAlreadyAllocated` when a
    /// stage-1 is already installed (use `stage_s1_hwpt` to replace it) and
    /// with `S1HwptAlreadyStaged` when a staged one is pending.
    pub fn allocate_s1_hwpt(&mut self, hwpt_data: &IommufdHwptData) -> Result<u32> {
        if self.s1_hwpt_id.is_some() {
            return Err(IommufdError::S1HwptAlreadyAllocated(self.vdevice_id));
        }
        let s1_hwpt_id = self.stage_s1_hwpt(hwpt_data)?;
        // Nothing is installed, so this is the promotion alone and cannot
        // fail: the only fallible step of a commit is destroying the old
        // stage-1.
        self.commit_staged_s1_hwpt()?;
        Ok(s1_hwpt_id)
    }

    /// Allocate a second stage-1 HWPT without disturbing the installed one.
    ///
    /// This is the first half of an atomic stage-1 replacement: the caller
    /// attaches the device to the returned id (the kernel services that as
    /// `iommufd_device_replace`, so the device is never without a
    /// translation), then calls `commit_staged_s1_hwpt` on success or
    /// `discard_staged_s1_hwpt` on failure. While a stage is pending the
    /// installed stage-1, if any, is untouched and stays attached. At most
    /// one stage may be pending: a second call fails with
    /// `S1HwptAlreadyStaged` before any ioctl is issued.
    pub fn stage_s1_hwpt(&mut self, hwpt_data: &IommufdHwptData) -> Result<u32> {
        if self.staged_s1_hwpt_id.is_some() {
            return Err(IommufdError::S1HwptAlreadyStaged(self.vdevice_id));
        }
        let staged = self.alloc_s1_hwpt_object(hwpt_data)?;
        self.staged_s1_hwpt_id = Some(staged);
        Ok(staged)
    }

    /// Promote the staged stage-1 HWPT to installed, then destroy the one it
    /// replaces.
    ///
    /// Call only once the device is attached to the staged HWPT: destroying
    /// the old one while the device is still attached to it fails with
    /// `EBUSY`. The promotion happens first and unconditionally, so after
    /// this call `s1_hwpt_id` names the HWPT the device is attached to even
    /// when the destroy fails. Any `Err` other than `S1HwptNotStaged`
    /// therefore means only that the OLD object could not be destroyed; it
    /// is then leaked until the iommufd context closes, and the caller
    /// should not retry or re-park the device on that account. The error
    /// does not carry the leaked id: a caller that wants to log it must
    /// read `s1_hwpt_id` before calling. Fails with `S1HwptNotStaged`,
    /// changing nothing, when no stage is pending.
    pub fn commit_staged_s1_hwpt(&mut self) -> Result<()> {
        let Some(staged) = self.staged_s1_hwpt_id.take() else {
            return Err(IommufdError::S1HwptNotStaged(self.vdevice_id));
        };
        // Promote first: the struct must never name a HWPT the device is no
        // longer attached to, whatever happens to the old object below.
        let old = self.s1_hwpt_id.replace(staged);
        match old {
            Some(old) => self.viommu.iommufd.destroy_iommu_object(old),
            None => Ok(()),
        }
    }

    /// Destroy the staged stage-1 HWPT, leaving the installed one alone.
    ///
    /// For the caller whose attach to the staged HWPT failed: the kernel
    /// makes no change to the attachment on a failed replace, so the
    /// installed stage-1 (if any) is still the one the device is on. The
    /// stage is cleared even when the destroy fails, so the vDevice can be
    /// staged again; the object is then leaked until the iommufd context
    /// closes. Fails with `S1HwptNotStaged`, changing nothing, when no stage
    /// is pending.
    pub fn discard_staged_s1_hwpt(&mut self) -> Result<()> {
        let Some(staged) = self.staged_s1_hwpt_id.take() else {
            return Err(IommufdError::S1HwptNotStaged(self.vdevice_id));
        };
        self.viommu.iommufd.destroy_iommu_object(staged)
    }

    /// Destroy the installed s1 hwpt for the vDevice. A pending stage is
    /// left as it is; use `discard_staged_s1_hwpt` for that.
    pub fn destroy_s1_hwpt(&mut self) -> Result<()> {
        if let Some(s1_hwpt_id) = self.s1_hwpt_id {
            self.viommu.iommufd.destroy_iommu_object(s1_hwpt_id)?;
            self.s1_hwpt_id = None;
        }
        Ok(())
    }

    /// Get device hardware information
    pub fn get_device_hw_info(
        &self,
        hw_info_data: &mut IommufdHwInfoData,
    ) -> Result<iommu_hw_info> {
        let mut hw_info = match hw_info_data {
            IommufdHwInfoData::Smmuv3(data) => iommu_hw_info {
                size: std::mem::size_of::<iommu_hw_info>() as u32,
                dev_id: self.dev_id,
                data_len: std::mem::size_of::<iommu_hw_info_arm_smmuv3>() as u32,
                data_uptr: data as *mut _ as u64,
                ..Default::default()
            },
            IommufdHwInfoData::Vtd(_) => {
                unimplemented!()
            }
            IommufdHwInfoData::Tegra241Cmdqv(data) => iommu_hw_info {
                size: std::mem::size_of::<iommu_hw_info>() as u32,
                flags: iommufd_hw_info_flags_IOMMU_HW_INFO_FLAG_INPUT_TYPE,
                dev_id: self.dev_id,
                data_len: std::mem::size_of::<iommu_hw_info_tegra241_cmdqv>() as u32,
                data_uptr: data as *mut _ as u64,
                __bindgen_anon_1: iommu_hw_info__bindgen_ty_1 {
                    in_data_type: iommu_hw_info_type_IOMMU_HW_INFO_TYPE_TEGRA241_CMDQV,
                },
                ..Default::default()
            },
        };

        self.viommu.iommufd.get_hw_info(&mut hw_info)?;

        Ok(hw_info)
    }
}

impl Drop for IommufdVDevice {
    fn drop(&mut self) {
        if let Err(e) = self.viommu.iommufd.destroy_iommu_object(self.vdevice_id) {
            eprintln!("Failed to destroy vDevice id {}: {}", self.vdevice_id, e);
        }

        if let Some(s1_hwpt_id) = self.s1_hwpt_id
            && let Err(e) = self.viommu.iommufd.destroy_iommu_object(s1_hwpt_id)
        {
            eprintln!("Failed to destroy s1_hwpt id {}: {}", s1_hwpt_id, e);
        }

        if let Some(staged) = self.staged_s1_hwpt_id
            && let Err(e) = self.viommu.iommufd.destroy_iommu_object(staged)
        {
            eprintln!("Failed to destroy staged s1_hwpt id {staged}: {e}");
        }
    }
}

impl AsRawFd for IommuFd {
    fn as_raw_fd(&self) -> RawFd {
        self.iommufd.as_raw_fd()
    }
}

ioctl_io_nr!(IOMMU_DESTROY, IOMMUFD_TYPE as u32, IOMMUFD_CMD_DESTROY);
ioctl_io_nr!(
    IOMMU_IOAS_ALLOC,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_IOAS_ALLOC
);
ioctl_io_nr!(IOMMU_IOAS_MAP, IOMMUFD_TYPE as u32, IOMMUFD_CMD_IOAS_MAP);
ioctl_io_nr!(
    IOMMU_IOAS_UNMAP,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_IOAS_UNMAP
);
ioctl_io_nr!(
    IOMMU_IOAS_MAP_FILE,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_IOAS_MAP_FILE
);
ioctl_io_nr!(
    IOMMU_IOAS_IOVA_RANGES,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_IOAS_IOVA_RANGES
);
ioctl_io_nr!(
    IOMMU_HWPT_ALLOC,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_HWPT_ALLOC
);
ioctl_io_nr!(
    IOMMUFD_GET_HW_INFO,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_GET_HW_INFO
);
ioctl_io_nr!(
    IOMMUFD_HWPT_INVALIDATE,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_HWPT_INVALIDATE
);
ioctl_io_nr!(
    IOMMU_VIOMMU_ALLOC,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_VIOMMU_ALLOC
);
ioctl_io_nr!(
    IOMMU_VDEVICE_ALLOC,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_VDEVICE_ALLOC
);
ioctl_io_nr!(
    IOMMU_VEVENTQ_ALLOC,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_VEVENTQ_ALLOC
);
ioctl_io_nr!(
    IOMMU_HW_QUEUE_ALLOC,
    IOMMUFD_TYPE as u32,
    IOMMUFD_CMD_HW_QUEUE_ALLOC
);

// Safety:
// - absolutely trust the underlying kernel
// - absolutely trust data returned by the underlying kernel
// - assume kernel will return error if caller passes in invalid file handle, parameter or buffer.
pub(crate) mod iommufd_syscall {
    use super::*;
    use vmm_sys_util::ioctl::{ioctl_with_mut_ref, ioctl_with_ref};

    pub(crate) fn destroy_iommufd(iommufd: &IommuFd, destroy_data: &iommu_destroy) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'destroy_data' points to initialized memory with expected data structure,
        // and remains valid for the duration of sysca
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_ref(iommufd, IOMMU_DESTROY(), destroy_data) };
        if ret < 0 {
            Err(IommufdError::IommuDestroy(SysError::last()))
        } else {
            Ok(())
        }
    }
    pub(crate) fn alloc_iommu_ioas(
        iommufd: &IommuFd,
        alloc_data: &mut iommu_ioas_alloc,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'alloc_data' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_IOAS_ALLOC(), alloc_data) };
        if ret < 0 {
            Err(IommufdError::IommuIoasAlloc(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn map_iommu_ioas(iommufd: &IommuFd, map: &iommu_ioas_map) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'map' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_ref(iommufd, IOMMU_IOAS_MAP(), map) };
        if ret < 0 {
            Err(IommufdError::IommuIoasMap(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn unmap_iommu_ioas(iommufd: &IommuFd, unmap: &mut iommu_ioas_unmap) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'unmap' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_IOAS_UNMAP(), unmap) };
        if ret < 0 {
            Err(IommufdError::IommuIoasUnmap(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn map_file_iommu_ioas(iommufd: &IommuFd, map: &iommu_ioas_map_file) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'map' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_ref(iommufd, IOMMU_IOAS_MAP_FILE(), map) };
        if ret < 0 {
            Err(IommufdError::IommuIoasMapFile(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn iova_ranges_iommu_ioas(
        iommufd: &IommuFd,
        ranges: &mut iommu_ioas_iova_ranges,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'ranges' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_IOAS_IOVA_RANGES(), ranges) };
        if ret < 0 {
            Err(IommufdError::IommuIoasIovaRanges(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn alloc_iommu_hwpt(
        iommufd: &IommuFd,
        hwpt_alloc: &mut iommu_hwpt_alloc,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'hwpt_alloc' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_HWPT_ALLOC(), hwpt_alloc) };
        if ret < 0 {
            Err(IommufdError::IommuHwptAlloc(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn get_hw_info(iommufd: &IommuFd, hw_info: &mut iommu_hw_info) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'hw_info' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMUFD_GET_HW_INFO(), hw_info) };
        if ret < 0 {
            Err(IommufdError::IommuGetHwInfo(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn invalidate_hwpt(
        iommufd: &IommuFd,
        hwpt_invalidate: &mut iommu_hwpt_invalidate,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'hwpt_invalidate' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret =
            unsafe { ioctl_with_mut_ref(iommufd, IOMMUFD_HWPT_INVALIDATE(), hwpt_invalidate) };
        if ret < 0 {
            Err(IommufdError::IommuHwptInvalidate(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn alloc_iommu_viommu(
        iommufd: &IommuFd,
        viommu_alloc: &mut iommu_viommu_alloc,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'viommu_alloc' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_VIOMMU_ALLOC(), viommu_alloc) };
        if ret < 0 {
            Err(IommufdError::IommuViommuAlloc(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn alloc_iommu_vdevice(
        iommufd: &IommuFd,
        vdevice_alloc: &mut iommu_vdevice_alloc,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'vdevice_alloc' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_VDEVICE_ALLOC(), vdevice_alloc) };
        if ret < 0 {
            Err(IommufdError::IommuVdeviceAlloc(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn alloc_iommu_veventq(
        iommufd: &IommuFd,
        veventq_alloc: &mut iommu_veventq_alloc,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'veventq_alloc' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_VEVENTQ_ALLOC(), veventq_alloc) };
        if ret < 0 {
            Err(IommufdError::IommuVeventqAlloc(SysError::last()))
        } else {
            Ok(())
        }
    }

    pub(crate) fn alloc_hw_queue(
        iommufd: &IommuFd,
        hw_queue_alloc: &mut iommu_hw_queue_alloc,
    ) -> Result<()> {
        // SAFETY:
        // 1. The file descriptor provided by 'iommufd' is valid and open.
        // 2. The 'hw_queue_alloc' points to initialized memory with expected data structure,
        // and remains valid for the duration of syscall.
        // 3. The return value is checked.
        let ret = unsafe { ioctl_with_mut_ref(iommufd, IOMMU_HW_QUEUE_ALLOC(), hw_queue_alloc) };
        if ret < 0 {
            Err(IommufdError::IommuHwQueueAlloc(SysError::last()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iommufd_ioctl_code() {
        assert_eq!(IOMMU_DESTROY(), 15232);
        assert_eq!(IOMMU_IOAS_ALLOC(), 15233);
        assert_eq!(IOMMU_IOAS_MAP(), 15237);
        assert_eq!(IOMMU_IOAS_UNMAP(), 15238);
        assert_eq!(IOMMU_IOAS_MAP_FILE(), 0x3b8f);
        assert_eq!(IOMMU_IOAS_IOVA_RANGES(), 0x3b84);
        assert_eq!(IOMMU_HWPT_ALLOC(), 15241);
        assert_eq!(IOMMUFD_GET_HW_INFO(), 15242);
        assert_eq!(IOMMUFD_HWPT_INVALIDATE(), 15245);
        assert_eq!(IOMMU_VIOMMU_ALLOC(), 15248);
        assert_eq!(IOMMU_VDEVICE_ALLOC(), 15249);
        assert_eq!(IOMMU_VEVENTQ_ALLOC(), 15251);
        assert_eq!(IOMMU_HW_QUEUE_ALLOC(), 0x3b94);
    }

    #[test]
    fn test_iommu_hw_queue_alloc_size() {
        assert_eq!(std::mem::size_of::<iommu_hw_queue_alloc>(), 40);
    }

    #[test]
    fn test_iommu_ioas_map_file_size() {
        assert_eq!(std::mem::size_of::<iommu_ioas_map_file>(), 40);
    }

    #[test]
    fn test_iommu_ioas_iova_ranges_size() {
        assert_eq!(std::mem::size_of::<iommu_ioas_iova_ranges>(), 32);
    }

    #[test]
    fn test_iommu_hw_info_tegra241_cmdqv_size() {
        assert_eq!(std::mem::size_of::<iommu_hw_info_tegra241_cmdqv>(), 8);
    }

    #[test]
    fn test_iommu_hw_info_type_tegra241_cmdqv() {
        assert_eq!(iommu_hw_info_type_IOMMU_HW_INFO_TYPE_TEGRA241_CMDQV, 3);
    }

    /// A vDevice whose iommufd is `/dev/null`, so every ioctl the verbs
    /// issue fails with ENOTTY and every verb that returns before its
    /// ioctl is distinguishable from one that does not. Wrapped in
    /// `ManuallyDrop` so the destroy-on-drop paths never run against it.
    fn null_backed_vdevice(
        s1_hwpt_id: Option<u32>,
        staged_s1_hwpt_id: Option<u32>,
    ) -> std::mem::ManuallyDrop<IommufdVDevice> {
        let iommufd = IommuFd::new_from_fd(File::open("/dev/null").unwrap());
        let viommu = IommufdVIommu {
            iommufd: Arc::new(iommufd),
            viommu_id: 1,
            dev_id: 2,
            s2_hwpt_id: 3,
            bypass_hwpt_id: None,
            abort_hwpt_id: None,
            cmdqv_data: None,
        };
        std::mem::ManuallyDrop::new(IommufdVDevice {
            viommu: Arc::new(viommu),
            dev_id: 2,
            virt_id: 0x99,
            vdevice_id: 42,
            s1_hwpt_id,
            staged_s1_hwpt_id,
        })
    }

    fn raw_hwpt_data() -> IommufdHwptData {
        IommufdHwptData::Raw {
            data_type: 0xdead,
            data: vec![0; 4],
        }
    }

    #[test]
    fn test_stage_refuses_a_second_stage_before_any_ioctl() {
        let mut vdev = null_backed_vdevice(Some(3), Some(7));
        // The guard must fire before IOMMU_HWPT_ALLOC is attempted: on
        // /dev/null that ioctl would surface as IommuHwptAlloc instead.
        assert!(matches!(
            vdev.stage_s1_hwpt(&raw_hwpt_data()),
            Err(IommufdError::S1HwptAlreadyStaged(42))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(3));
        assert_eq!(vdev.staged_s1_hwpt_id, Some(7));
    }

    #[test]
    fn test_allocate_refuses_when_a_stage1_is_installed() {
        let mut vdev = null_backed_vdevice(Some(3), None);
        assert!(matches!(
            vdev.allocate_s1_hwpt(&raw_hwpt_data()),
            Err(IommufdError::S1HwptAlreadyAllocated(42))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(3));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }

    #[test]
    fn test_commit_and_discard_refuse_without_a_stage() {
        let mut vdev = null_backed_vdevice(Some(3), None);
        assert!(matches!(
            vdev.commit_staged_s1_hwpt(),
            Err(IommufdError::S1HwptNotStaged(42))
        ));
        assert!(matches!(
            vdev.discard_staged_s1_hwpt(),
            Err(IommufdError::S1HwptNotStaged(42))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(3));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }

    #[test]
    fn test_commit_promotes_before_destroying_the_old_stage1() {
        let mut vdev = null_backed_vdevice(Some(3), Some(9));
        // The destroy of the old object (id 3) fails on /dev/null; the
        // promotion must already have happened regardless.
        assert!(matches!(
            vdev.commit_staged_s1_hwpt(),
            Err(IommufdError::IommuDestroy(_))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(9));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }

    #[test]
    fn test_commit_with_nothing_installed_is_promotion_only() {
        let mut vdev = null_backed_vdevice(None, Some(9));
        // No old object, so no IOMMU_DESTROY is issued: on /dev/null any
        // ioctl would have failed.
        assert!(vdev.commit_staged_s1_hwpt().is_ok());
        assert_eq!(vdev.s1_hwpt_id, Some(9));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }

    #[test]
    fn test_discard_clears_the_stage_and_keeps_the_installed_stage1() {
        let mut vdev = null_backed_vdevice(Some(3), Some(9));
        // The destroy of the staged object fails on /dev/null; the stage
        // is still cleared so the vDevice can be staged again, and the
        // installed stage-1 is untouched.
        assert!(matches!(
            vdev.discard_staged_s1_hwpt(),
            Err(IommufdError::IommuDestroy(_))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(3));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }
}
