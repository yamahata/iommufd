## Upcoming release

## Changed

## Added
- Add `IommuFd::map_file_iommu_ioas` (`IOMMU_IOAS_MAP_FILE`) to import a
  file-backed range into an IOAS.
- Add `IommuFd::iova_ranges_iommu_ioas` (`IOMMU_IOAS_IOVA_RANGES`) to read an
  IOAS's usable IOVA ranges and alignment.
- Add `IommufdVDevice::stage_s1_hwpt`, `commit_staged_s1_hwpt` and
  `discard_staged_s1_hwpt` (with the `staged_s1_hwpt_id` field and the
  `S1HwptAlreadyStaged` / `S1HwptNotStaged` errors) so a live stage-1 HWPT
  can be replaced with a single attach instead of a destroy-allocate-attach
  sequence.

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
