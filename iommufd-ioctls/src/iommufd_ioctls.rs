// Copyright © 2025 Crusoe Energy Systems LLC
//
// SPDX-License-Identifier: Apache-2.0
//

use std::fs::{File, OpenOptions};
use std::mem::ManuallyDrop;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::sync::Arc;

use iommufd_bindings::iommufd::*;
use vmm_sys_util::errno::Error as SysError;

use crate::{IommufdError, Result};

/// Identifiers of the kernel's iommufd selftest mock driver
/// (`CONFIG_IOMMUFD_TEST`). These come from
/// `drivers/iommu/iommufd/iommufd_test.h`, not from the public uAPI
/// headers, so bindgen does not generate them; they are defined here so a
/// test has one source of truth for them rather than its own copies.
pub const IOMMU_VIOMMU_TYPE_SELFTEST: u32 = 0xdead_beef;
/// `enum iommu_hwpt_data_type` value of the selftest mock driver.
pub const IOMMU_HWPT_DATA_SELFTEST: u32 = 0xdead;
/// `enum iommu_hwpt_invalidate_data_type` value of the selftest mock driver.
pub const IOMMU_VIOMMU_INVALIDATE_DATA_SELFTEST: u32 = 0xdead_beef;
/// `enum iommu_veventq_type` value of the selftest mock driver.
pub const IOMMU_VEVENTQ_TYPE_SELFTEST: u32 = 0xbeef_beef;
/// `enum iommu_hw_info_type` value of the selftest mock driver.
pub const IOMMU_HW_INFO_TYPE_SELFTEST: u32 = 0xfeed_beef;

/// `struct iommu_hwpt_selftest` — the mock driver's nested-HWPT payload.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct iommu_hwpt_selftest {
    pub iotlb: u32,
}

/// `struct iommu_viommu_invalidate_selftest` — the mock driver's
/// invalidation entry.
#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct iommu_viommu_invalidate_selftest {
    pub flags: u32,
    pub cache_id: u32,
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

    /// New from an already-opened `/dev/iommu` file. Caller transfers
    /// ownership; closing the fd is now `IommuFd`'s responsibility.
    pub fn new_from_fd(file: File) -> Self {
        IommuFd { iommufd: file }
    }

    pub fn destroy_iommu_object(&self, id: u32) -> Result<()> {
        let destroy_data = iommu_destroy {
            size: std::mem::size_of::<iommu_destroy>() as u32,
            id,
        };
        iommufd_syscall::destroy_iommu_object(self, &destroy_data)
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

    pub fn device_hw_info(
        &self,
        dev_id: u32,
        hw_info_data: &mut IommufdHwInfoData,
    ) -> Result<iommu_hw_info> {
        let mut hw_info = match hw_info_data {
            IommufdHwInfoData::Smmuv3(data) => iommu_hw_info {
                size: std::mem::size_of::<iommu_hw_info>() as u32,
                dev_id,
                data_len: std::mem::size_of::<iommu_hw_info_arm_smmuv3>() as u32,
                data_uptr: data as *mut _ as u64,
                ..Default::default()
            },
            IommufdHwInfoData::Vtd(_) => return Err(IommufdError::VtdUnsupported),
        };

        self.get_hw_info(&mut hw_info)?;

        Ok(hw_info)
    }

    pub fn alloc_veventq(&self, veventq_alloc: &mut iommu_veventq_alloc) -> Result<(u32, File)> {
        iommufd_syscall::alloc_veventq(self, veventq_alloc)?;

        // SAFETY: valid fd opened and owned by us.
        let file = unsafe { File::from_raw_fd(veventq_alloc.out_veventq_fd as RawFd) };

        // SAFETY: FFI call on an fd we own and the return value is checked.
        let ret = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) };
        if ret < 0 {
            return Err(IommufdError::VeventqNonBlocking(SysError::last()));
        }

        Ok((veventq_alloc.out_veventq_id, file))
    }

    pub fn alloc_hw_queue(&self, hw_queue_alloc: &mut iommu_hw_queue_alloc) -> Result<()> {
        iommufd_syscall::alloc_hw_queue(self, hw_queue_alloc)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum IommufdInvalidateData {
    Smmuv3(iommu_viommu_arm_smmuv3_invalidate),
    Vtd(iommu_hwpt_vtd_s1_invalidate),
    /// Invalidation entry for the kernel's iommufd selftest mock driver.
    Selftest(iommu_viommu_invalidate_selftest),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum IommuKind {
    Smmuv3,
    Smmuv3Cmdqv,
    Vtd,
    Amd,
    /// The kernel's own iommufd selftest mock driver
    /// (`CONFIG_IOMMUFD_TEST`). Never returned by [`IommuKind::query`] -
    /// the mock driver reports a hardware-info type no real IOMMU uses, so
    /// probing must not silently classify hardware as a mock. Reachable only
    /// by explicitly constructing a selftest vIOMMU.
    Selftest,
}

impl IommuKind {
    fn reports(iommufd: &IommuFd, dev_id: u32, hw_info_type: iommu_hw_info_type) -> bool {
        let mut hw_info = iommu_hw_info {
            size: std::mem::size_of::<iommu_hw_info>() as u32,
            flags: iommufd_hw_info_flags_IOMMU_HW_INFO_FLAG_INPUT_TYPE,
            dev_id,
            __bindgen_anon_1: iommu_hw_info__bindgen_ty_1 {
                in_data_type: hw_info_type,
            },
            ..Default::default()
        };

        iommufd.get_hw_info(&mut hw_info).is_ok()
    }

    #[allow(non_upper_case_globals)]
    fn query(iommufd: &IommuFd, dev_id: u32) -> Result<Self> {
        let mut hw_info = iommu_hw_info {
            size: std::mem::size_of::<iommu_hw_info>() as u32,
            dev_id,
            ..Default::default()
        };
        iommufd.get_hw_info(&mut hw_info)?;

        // SAFETY: the union holds one `__u32` under either name.
        let hw_info_type = unsafe { hw_info.__bindgen_anon_1.out_data_type };

        match hw_info_type {
            iommu_hw_info_type_IOMMU_HW_INFO_TYPE_TEGRA241_CMDQV => Ok(IommuKind::Smmuv3Cmdqv),
            iommu_hw_info_type_IOMMU_HW_INFO_TYPE_ARM_SMMUV3 => {
                if Self::reports(
                    iommufd,
                    dev_id,
                    iommu_hw_info_type_IOMMU_HW_INFO_TYPE_TEGRA241_CMDQV,
                ) {
                    Ok(IommuKind::Smmuv3Cmdqv)
                } else {
                    Ok(IommuKind::Smmuv3)
                }
            }
            iommu_hw_info_type_IOMMU_HW_INFO_TYPE_INTEL_VTD => Ok(IommuKind::Vtd),
            iommu_hw_info_type_IOMMU_HW_INFO_TYPE_AMD => Ok(IommuKind::Amd),
            _ => Err(IommufdError::UnknownIommu(hw_info_type)),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum IommufdViommuData {
    Smmuv3,
    Tegra241Cmdqv {
        mmap_offset: u64,
        mmap_length: u64,
    },
    /// A selftest vIOMMU carries no driver data.
    Selftest,
}

pub struct IommufdVIommu {
    iommufd: Arc<IommuFd>,
    viommu_id: u32,
    s2_hwpt_id: u32,
    bypass_hwpt_id: Option<u32>,
    abort_hwpt_id: Option<u32>,
    kind: IommuKind,
    data: IommufdViommuData,
}

impl IommufdVIommu {
    #[allow(non_upper_case_globals)]
    pub fn new(iommufd: Arc<IommuFd>, ioas_id: u32, dev_id: u32, hw_queue: bool) -> Result<Self> {
        let kind = IommuKind::query(&iommufd, dev_id)?;
        if hw_queue && kind != IommuKind::Smmuv3Cmdqv {
            return Err(IommufdError::HwQueueUnsupported);
        }

        let (viommu_type, bypass_s1_hwpt_data, abort_s1_hwpt_data) = match kind {
            IommuKind::Smmuv3 | IommuKind::Smmuv3Cmdqv => {
                const SMMU_STE_VALID: u64 = 1 << 0;
                const SMMU_STE_CFG_BYPASS: u64 = 1 << 3;

                (
                    if hw_queue {
                        iommu_viommu_type_IOMMU_VIOMMU_TYPE_TEGRA241_CMDQV
                    } else {
                        iommu_viommu_type_IOMMU_VIOMMU_TYPE_ARM_SMMUV3
                    },
                    IommufdHwptData::Smmuv3(iommu_hwpt_arm_smmuv3 {
                        ste: [SMMU_STE_CFG_BYPASS | SMMU_STE_VALID, 0x0],
                    }),
                    IommufdHwptData::Smmuv3(iommu_hwpt_arm_smmuv3 {
                        ste: [SMMU_STE_VALID, 0x0],
                    }),
                )
            }
            IommuKind::Vtd => return Err(IommufdError::VtdUnsupported),
            IommuKind::Amd => return Err(IommufdError::AmdUnsupported),
            // `new` probes the hardware, and `query` never reports Selftest;
            // a mock vIOMMU is built by `new_selftest`.
            IommuKind::Selftest => return Err(IommufdError::SelftestNotProbed),
        };

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

        let mut viommu_alloc = iommu_viommu_alloc {
            size: std::mem::size_of::<iommu_viommu_alloc>() as u32,
            type_: viommu_type,
            dev_id,
            hwpt_id: s2_hwpt_id,
            ..Default::default()
        };
        let data = match viommu_type {
            iommu_viommu_type_IOMMU_VIOMMU_TYPE_ARM_SMMUV3 => {
                iommufd.alloc_iommu_viommu(&mut viommu_alloc)?;
                IommufdViommuData::Smmuv3
            }
            iommu_viommu_type_IOMMU_VIOMMU_TYPE_TEGRA241_CMDQV => {
                let mut cmdqv_data = iommu_viommu_tegra241_cmdqv::default();
                viommu_alloc.data_len = std::mem::size_of::<iommu_viommu_tegra241_cmdqv>() as u32;
                viommu_alloc.data_uptr = &mut cmdqv_data as *mut iommu_viommu_tegra241_cmdqv as u64;
                iommufd.alloc_iommu_viommu(&mut viommu_alloc)?;
                IommufdViommuData::Tegra241Cmdqv {
                    mmap_offset: cmdqv_data.out_vintf_mmap_offset,
                    mmap_length: cmdqv_data.out_vintf_mmap_length,
                }
            }
            _ => return Err(IommufdError::UnsupportedViommuType(viommu_type)),
        };
        let viommu_id = viommu_alloc.out_viommu_id;

        let mut viommu = IommufdVIommu {
            iommufd,
            viommu_id,
            s2_hwpt_id,
            bypass_hwpt_id: None,
            abort_hwpt_id: None,
            kind,
            data,
        };
        viommu.bypass_hwpt_id = Some(viommu.alloc_s1_hwpt(dev_id, &bypass_s1_hwpt_data)?);
        viommu.abort_hwpt_id = Some(viommu.alloc_s1_hwpt(dev_id, &abort_s1_hwpt_data)?);

        Ok(viommu)
    }

    /// Allocate a vIOMMU backed by the kernel's iommufd selftest mock
    /// driver, for tests running against `CONFIG_IOMMUFD_TEST` with no
    /// hardware.
    ///
    /// Unlike [`IommufdVIommu::new`] this does not probe the device's
    /// hardware type - the mock driver is not real hardware and
    /// [`IommuKind::query`] deliberately refuses to report it - and it
    /// allocates no bypass or abort stage-1 tables, because the mock driver
    /// has no STE to describe them with. [`IommufdVIommu::attach_bypass`]
    /// and [`IommufdVIommu::attach_abort`] therefore must not be called on
    /// the result.
    pub fn new_selftest(iommufd: Arc<IommuFd>, ioas_id: u32, dev_id: u32) -> Result<Self> {
        let mut s2_hwpt_alloc = iommu_hwpt_alloc {
            size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
            flags: iommufd_hwpt_alloc_flags_IOMMU_HWPT_ALLOC_NEST_PARENT,
            dev_id,
            pt_id: ioas_id,
            data_type: iommu_hwpt_data_type_IOMMU_HWPT_DATA_NONE,
            ..Default::default()
        };
        iommufd.alloc_iommu_hwpt(&mut s2_hwpt_alloc)?;
        let s2_hwpt_id = s2_hwpt_alloc.out_hwpt_id;

        let mut viommu_alloc = iommu_viommu_alloc {
            size: std::mem::size_of::<iommu_viommu_alloc>() as u32,
            type_: IOMMU_VIOMMU_TYPE_SELFTEST,
            dev_id,
            hwpt_id: s2_hwpt_id,
            ..Default::default()
        };
        iommufd.alloc_iommu_viommu(&mut viommu_alloc)?;

        Ok(IommufdVIommu {
            iommufd,
            viommu_id: viommu_alloc.out_viommu_id,
            s2_hwpt_id,
            bypass_hwpt_id: None,
            abort_hwpt_id: None,
            kind: IommuKind::Selftest,
            data: IommufdViommuData::Selftest,
        })
    }

    pub fn alloc_s1_hwpt(&self, dev_id: u32, hwpt_data: &IommufdHwptData) -> Result<u32> {
        let (data_type, data_len, data_uptr) = match (self.kind, hwpt_data) {
            (IommuKind::Smmuv3 | IommuKind::Smmuv3Cmdqv, IommufdHwptData::Smmuv3(data)) => (
                iommu_hwpt_data_type_IOMMU_HWPT_DATA_ARM_SMMUV3,
                std::mem::size_of::<iommu_hwpt_arm_smmuv3>() as u32,
                data as *const iommu_hwpt_arm_smmuv3 as u64,
            ),
            (IommuKind::Selftest, IommufdHwptData::Selftest(data)) => (
                IOMMU_HWPT_DATA_SELFTEST,
                std::mem::size_of::<iommu_hwpt_selftest>() as u32,
                data as *const iommu_hwpt_selftest as u64,
            ),
            (IommuKind::Vtd, IommufdHwptData::Vtd(_)) => return Err(IommufdError::VtdUnsupported),
            (kind, _) => return Err(IommufdError::IommuTypeMismatch(kind)),
        };

        let mut hwpt_alloc = iommu_hwpt_alloc {
            size: std::mem::size_of::<iommu_hwpt_alloc>() as u32,
            dev_id,
            pt_id: self.viommu_id,
            data_type,
            data_len,
            data_uptr,
            ..Default::default()
        };
        self.iommufd.alloc_iommu_hwpt(&mut hwpt_alloc)?;

        Ok(hwpt_alloc.out_hwpt_id)
    }

    pub fn data(&self) -> IommufdViommuData {
        self.data
    }

    /// The kernel's id for this vIOMMU object.
    pub fn viommu_id(&self) -> u32 {
        self.viommu_id
    }

    /// The kernel's id for the stage-2 nesting-parent HWPT this vIOMMU was
    /// allocated against.
    pub fn s2_hwpt_id(&self) -> u32 {
        self.s2_hwpt_id
    }

    pub fn iommufd(&self) -> &Arc<IommuFd> {
        &self.iommufd
    }

    pub fn attach_bypass(&self, device: &dyn AttachHwpt) -> Result<()> {
        let bypass_hwpt_id = self
            .bypass_hwpt_id
            .ok_or(IommufdError::NoParkingTable(self.viommu_id))?;
        self.attach(device, bypass_hwpt_id)
    }

    pub fn attach_abort(&self, device: &dyn AttachHwpt) -> Result<()> {
        let abort_hwpt_id = self
            .abort_hwpt_id
            .ok_or(IommufdError::NoParkingTable(self.viommu_id))?;
        self.attach(device, abort_hwpt_id)
    }

    fn attach(&self, device: &dyn AttachHwpt, hwpt_id: u32) -> Result<()> {
        device
            .attach_hwpt(hwpt_id)
            .map_err(IommufdError::AttachHwpt)
    }

    pub fn invalidate(&self, cmd: &mut IommufdInvalidateData) -> Result<bool> {
        match (self.kind, cmd) {
            (IommuKind::Smmuv3 | IommuKind::Smmuv3Cmdqv, IommufdInvalidateData::Smmuv3(data)) => {
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
                self.iommufd.invalidate_hwpt(&mut hw_invalidate)?;

                if hw_invalidate.entry_num == 1 {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            (IommuKind::Selftest, IommufdInvalidateData::Selftest(data)) => {
                let mut hw_invalidate = iommu_hwpt_invalidate {
                    size: std::mem::size_of::<iommu_hwpt_invalidate>() as u32,
                    hwpt_id: self.viommu_id,
                    data_type: IOMMU_VIOMMU_INVALIDATE_DATA_SELFTEST,
                    entry_len: std::mem::size_of::<iommu_viommu_invalidate_selftest>() as u32,
                    entry_num: 1,
                    data_uptr: data as *mut iommu_viommu_invalidate_selftest as u64,
                    ..Default::default()
                };
                self.iommufd.invalidate_hwpt(&mut hw_invalidate)?;

                Ok(hw_invalidate.entry_num == 1)
            }
            (IommuKind::Vtd, IommufdInvalidateData::Vtd(_)) => Err(IommufdError::VtdUnsupported),
            (kind, _) => Err(IommufdError::IommuTypeMismatch(kind)),
        }
    }

    pub fn hw_queue_support(&self) -> bool {
        matches!(self.data, IommufdViommuData::Tegra241Cmdqv { .. })
    }

    pub fn allocate_veventq(self: &Arc<Self>, depth: u32) -> Result<IommufdVEventQ> {
        let veventq_type = match self.kind {
            IommuKind::Smmuv3 | IommuKind::Smmuv3Cmdqv => {
                iommu_veventq_type_IOMMU_VEVENTQ_TYPE_ARM_SMMUV3
            }
            IommuKind::Vtd => return Err(IommufdError::VtdUnsupported),
            IommuKind::Amd => return Err(IommufdError::AmdUnsupported),
            IommuKind::Selftest => return Err(IommufdError::SelftestNotProbed),
        };
        self.alloc_veventq(veventq_type, depth)
    }

    /// Allocate a vEVENTQ of the selftest mock driver's type.
    ///
    /// Mirrors [`IommufdVIommu::allocate_cmdqv_veventq`]: the type is not
    /// inferable from the vIOMMU's kind, so it gets its own constructor
    /// rather than widening [`IommufdVIommu::allocate_veventq`].
    pub fn allocate_selftest_veventq(self: &Arc<Self>, depth: u32) -> Result<IommufdVEventQ> {
        if self.kind != IommuKind::Selftest {
            return Err(IommufdError::IommuTypeMismatch(self.kind));
        }
        self.alloc_veventq(IOMMU_VEVENTQ_TYPE_SELFTEST, depth)
    }

    pub fn allocate_cmdqv_veventq(self: &Arc<Self>, depth: u32) -> Result<IommufdVEventQ> {
        let veventq_type = iommu_veventq_type_IOMMU_VEVENTQ_TYPE_TEGRA241_CMDQV;
        if !matches!(self.data, IommufdViommuData::Tegra241Cmdqv { .. }) {
            return Err(IommufdError::UnsupportedVeventqType(veventq_type));
        }
        self.alloc_veventq(veventq_type, depth)
    }

    fn alloc_veventq(
        self: &Arc<Self>,
        veventq_type: iommu_veventq_type,
        depth: u32,
    ) -> Result<IommufdVEventQ> {
        let mut veventq_alloc = iommu_veventq_alloc {
            size: std::mem::size_of::<iommu_veventq_alloc>() as u32,
            viommu_id: self.viommu_id,
            type_: veventq_type,
            veventq_depth: depth,
            ..Default::default()
        };
        let (veventq_id, file) = self.iommufd.alloc_veventq(&mut veventq_alloc)?;

        Ok(IommufdVEventQ {
            viommu: Arc::clone(self),
            veventq_id,
            veventq_type,
            file: ManuallyDrop::new(file),
            last_sequence: None,
        })
    }

    pub fn allocate_hw_queue(
        self: &Arc<Self>,
        index: u32,
        base_addr: u64,
        length: u64,
    ) -> Result<IommufdHwQueue> {
        let hw_queue_type = match self.data {
            IommufdViommuData::Tegra241Cmdqv { .. } => {
                iommu_hw_queue_type_IOMMU_HW_QUEUE_TYPE_TEGRA241_CMDQV
            }
            IommufdViommuData::Smmuv3 | IommufdViommuData::Selftest => {
                return Err(IommufdError::HwQueueUnsupported);
            }
        };

        let mut hw_queue_alloc = iommu_hw_queue_alloc {
            size: std::mem::size_of::<iommu_hw_queue_alloc>() as u32,
            viommu_id: self.viommu_id,
            type_: hw_queue_type,
            index,
            nesting_parent_iova: base_addr,
            length,
            ..Default::default()
        };
        self.iommufd.alloc_hw_queue(&mut hw_queue_alloc)?;

        Ok(IommufdHwQueue {
            viommu: Arc::clone(self),
            hw_queue_id: hw_queue_alloc.out_hw_queue_id,
        })
    }
}

pub struct IommufdHwQueue {
    viommu: Arc<IommufdVIommu>,
    hw_queue_id: u32,
}

impl Drop for IommufdHwQueue {
    fn drop(&mut self) {
        if let Err(e) = self.viommu.iommufd.destroy_iommu_object(self.hw_queue_id) {
            log::error!("Failed to destroy HW queue id {}: {e}", self.hw_queue_id);
        }
    }
}

impl Drop for IommufdVIommu {
    fn drop(&mut self) {
        for (id, what) in [
            (self.bypass_hwpt_id, "bypass_hwpt"),
            (self.abort_hwpt_id, "abort_hwpt"),
            (Some(self.viommu_id), "vIOMMU"),
            (Some(self.s2_hwpt_id), "s2_hwpt"),
        ] {
            let Some(id) = id else { continue };
            if let Err(e) = self.iommufd.destroy_iommu_object(id) {
                log::error!("Failed to destroy {what} id {id}: {e}");
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum IommufdVEventData {
    Smmuv3(iommu_vevent_arm_smmuv3),
    Tegra241Cmdqv(iommu_vevent_tegra241_cmdqv),
}

/// A decoded virtual event.
#[derive(Debug, Copy, Clone)]
pub struct IommufdVEvent {
    pub header: iommufd_vevent_header,
    pub data: Option<IommufdVEventData>,
    pub lost: u32,
}

fn vevent_record_len(veventq_type: iommu_veventq_type) -> Option<usize> {
    if veventq_type == iommu_veventq_type_IOMMU_VEVENTQ_TYPE_ARM_SMMUV3 {
        Some(std::mem::size_of::<iommu_vevent_arm_smmuv3>())
    } else if veventq_type == iommu_veventq_type_IOMMU_VEVENTQ_TYPE_TEGRA241_CMDQV {
        Some(std::mem::size_of::<iommu_vevent_tegra241_cmdqv>())
    } else {
        None
    }
}

fn decode_vevent_record(
    veventq_type: iommu_veventq_type,
    bytes: &[u8],
) -> Option<IommufdVEventData> {
    // SAFETY: the records are `#[repr(C)]` PODs with no invalid bit pattern,
    // the caller guarantees `bytes` covers one, and `bytes` may be unaligned.
    unsafe {
        if veventq_type == iommu_veventq_type_IOMMU_VEVENTQ_TYPE_ARM_SMMUV3 {
            Some(IommufdVEventData::Smmuv3(std::ptr::read_unaligned(
                bytes.as_ptr() as *const iommu_vevent_arm_smmuv3,
            )))
        } else if veventq_type == iommu_veventq_type_IOMMU_VEVENTQ_TYPE_TEGRA241_CMDQV {
            Some(IommufdVEventData::Tegra241Cmdqv(std::ptr::read_unaligned(
                bytes.as_ptr() as *const iommu_vevent_tegra241_cmdqv,
            )))
        } else {
            None
        }
    }
}

pub struct IommufdVEventQ {
    viommu: Arc<IommufdVIommu>,
    veventq_id: u32,
    veventq_type: iommu_veventq_type,
    file: ManuallyDrop<File>,
    last_sequence: Option<u32>,
}

impl IommufdVEventQ {
    pub fn as_raw_fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    pub fn veventq_type(&self) -> iommu_veventq_type {
        self.veventq_type
    }

    pub fn read_events(&mut self) -> std::io::Result<Vec<IommufdVEvent>> {
        use std::io::Read;

        const HDR: usize = std::mem::size_of::<iommufd_vevent_header>();
        let rec = vevent_record_len(self.veventq_type).unwrap_or(0);

        let mut buf = vec![0u8; (HDR + rec) * 64];
        let n = self.file.read(&mut buf)?;

        let mut events = Vec::new();
        let mut off = 0;
        while off + HDR <= n {
            // SAFETY: `iommufd_vevent_header` is a `#[repr(C)]` POD read from
            // an inbound slice; `buf` has no alignment guarantee.
            let header: iommufd_vevent_header =
                unsafe { std::ptr::read_unaligned(buf[off..].as_ptr() as *const _) };
            off += HDR;

            const SEQ_SPAN: u32 = i32::MAX as u32 + 1;
            let lost = match self.last_sequence {
                Some(prev) => {
                    (header.sequence.wrapping_sub(prev) & (SEQ_SPAN - 1)).saturating_sub(1)
                }
                None => 0,
            };
            self.last_sequence = Some(header.sequence);

            if header.flags & iommu_veventq_flag_IOMMU_VEVENTQ_FLAG_LOST_EVENTS != 0 {
                events.push(IommufdVEvent {
                    header,
                    data: None,
                    lost,
                });
                continue;
            }

            if off + rec > n {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "vEVENTQ returned a truncated record",
                ));
            }
            let data = decode_vevent_record(self.veventq_type, &buf[off..]);
            off += rec;
            events.push(IommufdVEvent { header, data, lost });
        }

        Ok(events)
    }
}

impl Drop for IommufdVEventQ {
    fn drop(&mut self) {
        // SAFETY: `file` is live and never dropped anywhere else.
        unsafe { ManuallyDrop::drop(&mut self.file) };

        if let Err(e) = self.viommu.iommufd.destroy_iommu_object(self.veventq_id) {
            log::error!("Failed to destroy vEVENTQ id {}: {e}", self.veventq_id);
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum IommufdHwInfoData {
    Smmuv3(iommu_hw_info_arm_smmuv3),
    Vtd(iommu_hw_info_vtd),
}

#[derive(Debug, Copy, Clone)]
pub enum IommufdHwptData {
    Smmuv3(iommu_hwpt_arm_smmuv3),
    Vtd(iommu_hwpt_vtd_s1),
    /// Nested-HWPT data for the kernel's iommufd selftest mock driver.
    Selftest(iommu_hwpt_selftest),
}

/// A device attachable to an iommufd page table.
pub trait AttachHwpt: Send + Sync {
    /// Attach the device to the HW page table.
    fn attach_hwpt(&self, pt_id: u32) -> std::io::Result<()>;
}

pub struct IommufdVDevice {
    viommu: Arc<IommufdVIommu>,
    dev_id: u32,
    virt_id: u64,
    vdevice_id: u32,
    s1_hwpt_id: Option<u32>,
    /// A second stage-1 HWPT allocated but not yet installed, holding the
    /// first half of an atomic stage-1 replacement. At most one may be
    /// pending; see [`IommufdVDevice::stage_s1_hwpt`].
    staged_s1_hwpt_id: Option<u32>,
}

impl IommufdVDevice {
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

    fn allocate_s1_hwpt(&mut self, hwpt_data: &IommufdHwptData) -> Result<u32> {
        if self.s1_hwpt_id.is_some() {
            return Err(IommufdError::S1HwptAlreadyAllocated(self.vdevice_id));
        }
        // A pending stage owns the replacement in flight; installing
        // underneath it would leave `commit_staged_s1_hwpt` destroying an
        // object the device had just been attached to.
        if self.staged_s1_hwpt_id.is_some() {
            return Err(IommufdError::S1HwptAlreadyStaged(self.vdevice_id));
        }

        let s1_hwpt_id = self.viommu.alloc_s1_hwpt(self.dev_id, hwpt_data)?;
        self.s1_hwpt_id = Some(s1_hwpt_id);

        Ok(s1_hwpt_id)
    }

    /// Allocate a second stage-1 HWPT without disturbing the installed one.
    ///
    /// This is the first half of an atomic stage-1 replacement: the caller
    /// attaches the device to the returned id (the kernel services that as
    /// `iommufd_device_replace`, so the device is never without a
    /// translation), then calls [`IommufdVDevice::commit_staged_s1_hwpt`] on
    /// success or [`IommufdVDevice::discard_staged_s1_hwpt`] on failure.
    /// While a stage is pending the installed stage-1, if any, is untouched
    /// and stays attached. At most one stage may be pending: a second call
    /// fails with `S1HwptAlreadyStaged` before any ioctl is issued.
    pub fn stage_s1_hwpt(&mut self, hwpt_data: &IommufdHwptData) -> Result<u32> {
        if self.staged_s1_hwpt_id.is_some() {
            return Err(IommufdError::S1HwptAlreadyStaged(self.vdevice_id));
        }

        let staged = self.viommu.alloc_s1_hwpt(self.dev_id, hwpt_data)?;
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
    /// should not retry or re-park the device on that account. Fails with
    /// `S1HwptNotStaged`, changing nothing, when no stage is pending.
    pub fn commit_staged_s1_hwpt(&mut self) -> Result<()> {
        let Some(staged) = self.staged_s1_hwpt_id.take() else {
            return Err(IommufdError::S1HwptNotStaged(self.vdevice_id));
        };

        // Promote first: the struct must never name a HWPT the device is no
        // longer attached to, whatever happens to the old object below.
        match self.s1_hwpt_id.replace(staged) {
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

    fn destroy_s1_hwpt(&mut self) -> Result<()> {
        if let Some(s1_hwpt_id) = self.s1_hwpt_id {
            self.viommu.iommufd.destroy_iommu_object(s1_hwpt_id)?;
            self.s1_hwpt_id = None;
        }
        Ok(())
    }

    pub fn install_s1_hwpt(
        &mut self,
        device: &dyn AttachHwpt,
        hwpt_data: &IommufdHwptData,
    ) -> Result<()> {
        let s1_hwpt_id = self.allocate_s1_hwpt(hwpt_data)?;

        device
            .attach_hwpt(s1_hwpt_id)
            .map_err(IommufdError::AttachHwpt)?;

        Ok(())
    }

    pub fn uninstall_s1_hwpt(&mut self, device: &dyn AttachHwpt, abort: bool) -> Result<()> {
        if abort {
            self.viommu.attach_abort(device)?;
        } else {
            self.viommu.attach_bypass(device)?;
        }

        self.destroy_s1_hwpt()
    }

    pub fn hw_info(&self, hw_info_data: &mut IommufdHwInfoData) -> Result<iommu_hw_info> {
        match (self.viommu.kind, &hw_info_data) {
            (IommuKind::Smmuv3 | IommuKind::Smmuv3Cmdqv, IommufdHwInfoData::Smmuv3(_)) => {}
            (IommuKind::Vtd, IommufdHwInfoData::Vtd(_)) => {
                return Err(IommufdError::VtdUnsupported);
            }
            (kind, _) => return Err(IommufdError::IommuTypeMismatch(kind)),
        }

        self.viommu
            .iommufd
            .device_hw_info(self.dev_id, hw_info_data)
    }

    /// The kernel's id for the stage-1 HWPT installed on this vDevice,
    /// or `None` if none is installed.
    pub fn s1_hwpt_id(&self) -> Option<u32> {
        self.s1_hwpt_id
    }

    /// The kernel's id for the stage-1 HWPT staged on this vDevice, or
    /// `None` if nothing is staged.
    pub fn staged_s1_hwpt_id(&self) -> Option<u32> {
        self.staged_s1_hwpt_id
    }

    pub fn virt_id(&self) -> u64 {
        self.virt_id
    }
}

impl Drop for IommufdVDevice {
    fn drop(&mut self) {
        if let Some(s1_hwpt_id) = self.s1_hwpt_id
            && let Err(e) = self.viommu.iommufd.destroy_iommu_object(s1_hwpt_id)
        {
            log::error!("Failed to destroy s1_hwpt id {s1_hwpt_id}: {e}");
        }

        // A stage the caller neither committed nor discarded would otherwise
        // leak until the iommufd context closes.
        if let Some(staged) = self.staged_s1_hwpt_id
            && let Err(e) = self.viommu.iommufd.destroy_iommu_object(staged)
        {
            log::error!("Failed to destroy staged s1_hwpt id {staged}: {e}");
        }

        if let Err(e) = self.viommu.iommufd.destroy_iommu_object(self.vdevice_id) {
            log::error!("Failed to destroy vDevice id {}: {e}", self.vdevice_id);
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

    pub(crate) fn destroy_iommu_object(
        iommufd: &IommuFd,
        destroy_data: &iommu_destroy,
    ) -> Result<()> {
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

    pub(crate) fn alloc_veventq(
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
        assert_eq!(IOMMU_IOAS_MAP_FILE(), 15247);
        assert_eq!(IOMMU_IOAS_IOVA_RANGES(), 15236);
        assert_eq!(IOMMU_HWPT_ALLOC(), 15241);
        assert_eq!(IOMMUFD_GET_HW_INFO(), 15242);
        assert_eq!(IOMMUFD_HWPT_INVALIDATE(), 15245);
        assert_eq!(IOMMU_VIOMMU_ALLOC(), 15248);
        assert_eq!(IOMMU_VDEVICE_ALLOC(), 15249);
        assert_eq!(IOMMU_VEVENTQ_ALLOC(), 15251);
        assert_eq!(IOMMU_HW_QUEUE_ALLOC(), 15252);
    }

    #[test]
    fn test_iommu_ioas_map_file_size() {
        assert_eq!(std::mem::size_of::<iommu_ioas_map_file>(), 40);
    }

    #[test]
    fn test_iommu_ioas_iova_ranges_size() {
        assert_eq!(std::mem::size_of::<iommu_ioas_iova_ranges>(), 32);
    }

    /// A vDevice whose iommufd is `/dev/null`, so every ioctl the verbs
    /// issue fails with `ENOTTY` and a verb that returns BEFORE its ioctl is
    /// distinguishable from one that returns after it. Wrapped in
    /// `ManuallyDrop` so the destroy-on-drop paths never run against it.
    fn null_backed_vdevice(
        s1_hwpt_id: Option<u32>,
        staged_s1_hwpt_id: Option<u32>,
    ) -> ManuallyDrop<IommufdVDevice> {
        let iommufd = IommuFd::new_from_fd(File::open("/dev/null").unwrap());
        let viommu = IommufdVIommu {
            iommufd: Arc::new(iommufd),
            viommu_id: 1,
            s2_hwpt_id: 3,
            bypass_hwpt_id: None,
            abort_hwpt_id: None,
            kind: IommuKind::Smmuv3,
            data: IommufdViommuData::Smmuv3,
        };
        ManuallyDrop::new(IommufdVDevice {
            viommu: Arc::new(viommu),
            dev_id: 2,
            virt_id: 0x99,
            vdevice_id: 42,
            s1_hwpt_id,
            staged_s1_hwpt_id,
        })
    }

    fn smmuv3_hwpt_data() -> IommufdHwptData {
        IommufdHwptData::Smmuv3(iommu_hwpt_arm_smmuv3::default())
    }

    #[test]
    fn test_stage_refuses_a_second_stage_before_any_ioctl() {
        let mut vdev = null_backed_vdevice(Some(3), Some(7));
        // The guard must fire before IOMMU_HWPT_ALLOC is attempted: on
        // /dev/null that ioctl would surface as a different error.
        assert!(matches!(
            vdev.stage_s1_hwpt(&smmuv3_hwpt_data()),
            Err(IommufdError::S1HwptAlreadyStaged(42))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(3));
        assert_eq!(vdev.staged_s1_hwpt_id, Some(7));
    }

    #[test]
    fn test_allocate_refuses_while_a_stage_is_pending() {
        let mut vdev = null_backed_vdevice(None, Some(7));
        assert!(matches!(
            vdev.allocate_s1_hwpt(&smmuv3_hwpt_data()),
            Err(IommufdError::S1HwptAlreadyStaged(42))
        ));
        assert_eq!(vdev.s1_hwpt_id, None);
        assert_eq!(vdev.staged_s1_hwpt_id, Some(7));
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
        // promotion must already have happened regardless. Match the destroy
        // error specifically, so a guard misfiring here cannot pass.
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
        // ioctl would have failed, so Ok proves none was attempted.
        assert!(vdev.commit_staged_s1_hwpt().is_ok());
        assert_eq!(vdev.s1_hwpt_id, Some(9));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }

    #[test]
    fn test_discard_clears_the_stage_and_leaves_the_installed_one() {
        let mut vdev = null_backed_vdevice(Some(3), Some(9));
        // The destroy fails on /dev/null, but the stage must be cleared
        // anyway so the vDevice can be staged again.
        assert!(matches!(
            vdev.discard_staged_s1_hwpt(),
            Err(IommufdError::IommuDestroy(_))
        ));
        assert_eq!(vdev.s1_hwpt_id, Some(3));
        assert_eq!(vdev.staged_s1_hwpt_id, None);
    }
}
