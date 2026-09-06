// Copyright © 2025 Crusoe Energy Systems LLC
//
// SPDX-License-Identifier: Apache-2.0
//

#[macro_use]
extern crate vmm_sys_util;

use std::io;
use thiserror::Error;
use vmm_sys_util::errno::Error as SysError;

pub mod iommufd_ioctls;

pub use iommufd_ioctls::*;

use iommufd_bindings::iommufd::*;

#[derive(Debug, Error)]
pub enum IommufdError {
    #[error("failed to open /dev/iommufd: {0}")]
    OpenIommufd(#[source] io::Error),
    #[error("failed to destroy iommufd: {0}")]
    IommuDestroy(#[source] SysError),
    #[error("failed to allocate IOAS: {0}")]
    IommuIoasAlloc(#[source] SysError),
    #[error("failed to map an IOVA range to the IOAS: {0}")]
    IommuIoasMap(#[source] SysError),
    #[error("failed to unmap an IOVA range from the IOAS: {0}")]
    IommuIoasUnmap(#[source] SysError),
    #[error("failed to map a file-backed IOVA range to the IOAS: {0}")]
    IommuIoasMapFile(#[source] SysError),
    #[error("failed to query the IOAS IOVA ranges: {0}")]
    IommuIoasIovaRanges(#[source] SysError),
    #[error("failed to allocate HWPT: {0}")]
    IommuHwptAlloc(#[source] SysError),
    #[error("failed to allocate vIOMMU: {0}")]
    IommuViommuAlloc(#[source] SysError),
    #[error("failed to allocate vDevice: {0}")]
    IommuVdeviceAlloc(#[source] SysError),
    #[error("failed to allocate vEVENTQ: {0}")]
    IommuVeventqAlloc(#[source] SysError),
    #[error("failed to allocate HW queue: {0}")]
    IommuHwQueueAlloc(#[source] SysError),
    #[error("invalid invalidate data")]
    InvalidInvalidateData,
    #[error("failed to get HW info: {0}")]
    IommuGetHwInfo(#[source] SysError),
    #[error("failed to invalidate HWPT: {0}")]
    IommuHwptInvalidate(#[source] SysError),
    #[error("unsupported S1 HWPT data type: {0}")]
    UnsupportedS1HwptDataType(iommu_hwpt_data_type),
    #[error("S1 HWPT already allocated with for the given vDevice: {0}")]
    S1HwptAlreadyAllocated(u32),
    #[error("an S1 HWPT is already staged for the given vDevice: {0}")]
    S1HwptAlreadyStaged(u32),
    #[error("no S1 HWPT is staged for the given vDevice: {0}")]
    S1HwptNotStaged(u32),
}

pub type Result<T> = std::result::Result<T, IommufdError>;
