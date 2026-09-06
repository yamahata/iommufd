# iommufd

The `iommufd` workspace hosts libraries related to Rust bindings and
wrappers to the
[IOMMUFD](https://docs.kernel.org/userspace-api/iommufd.html) subsystem
from the Linux kernel. It currently consists of the following crates:

- `iommufd-bindings` -> Rust FFI bindings to IOMMUFD generated using bindgen
- `iommufd-ioctls` -> Safe wrappers over IOMMUFD uAPIs
- `mock-tests` -> An end-to-end exerciser for those wrappers against the
  kernel's iommufd selftest mock driver (`CONFIG_IOMMUFD_TEST`), needing no
  hardware. Not published; see `mock-tests/README.md`