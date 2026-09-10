## Upcoming release

## Changed
- Report destroy failures in `Drop` through the `log` crate instead of
  writing to stderr with `eprintln!`.

## Added
- Add `IommuFd::map_file_iommu_ioas`, wrapping `IOMMU_IOAS_MAP_FILE` to
  map a file-backed range into an IOAS.
- Add `IommuFd::iova_ranges_iommu_ioas`, wrapping `IOMMU_IOAS_IOVA_RANGES`
  to query the allowed IOVA ranges and minimum IOVA alignment.
- Add support for the kernel's iommufd selftest mock driver
  (`CONFIG_IOMMUFD_TEST`): `IommuKind::Selftest`,
  `IommufdVIommu::new_selftest`, `allocate_selftest_veventq`, the
  `IommufdHwptData::Selftest` and `IommufdInvalidateData::Selftest`
  payload variants, and the `viommu_id`/`s2_hwpt_id` accessors.

## Fixed

# [v0.3.0]

## Changed
- Require `iommufd-bindings` 0.2.0.

## Added
- Add `Iommufd` wrappers for the vIOMMU, vDevice, vEVENTQ and HW queue ioctls.
- Add `IommufdVIommu` and `IommufdVDevice`, which own the lifetime of a vIOMMU,
  its stage 2 HWPT and the per device stage 1 HWPTs, and expose invalidation and
  hardware info queries.
- Add `IommufdVEventQ` and ARM SMMUv3 event decoding.
- Add the `AttachHwpt` trait so a caller can attach a device to a stage 1
  HWPT without depending on how the device was opened.
- Add `IommufdHwQueue` for hardware accelerated command queues, enabled with
  the `hw_queue` flag through `IommufdVIommu::new`.

## Fixed

# [v0.2.0]

## Changed

## Added
- [[9]](https://github.com/cloud-hypervisor/iommufd/pull/9) Add `Iommufd::destroy_iommu_object` to release iommufd
  objects explicitly.
- [[9]](https://github.com/cloud-hypervisor/iommufd/pull/9) Add `Iommufd::new_from_fd` to construct a `Iommufd` from
  a pre-opened iommufd file.

## Fixed

# [v0.1.0]

This is the first `iommufd-ioctls` crate release.

The iommufd-ioctls crate provides safe wrappers over the
[IOMMUFD uAPIs](https://docs.kernel.org/userspace-api/iommufd.html#iommufd-user-api), a set
of ioctls used to control the IOMMU subsystem as it relates to managing
IO page tables from userspace. The ioctls are accessible through
structure `IommuFd`.
