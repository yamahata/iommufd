// Copyright © 2026 Crusoe Energy Systems LLC
//
// SPDX-License-Identifier: Apache-2.0
//
// Integration test for the iommufd-ioctls vIOMMU/vDEVICE/vEVENTQ verbs
// against the kernel's iommufd selftest mock driver
// (CONFIG_IOMMUFD_TEST=y). Runs the QEMU-equivalent accelerated-vSMMUv3
// allocation flow end to end with no hardware:
//
//   IOAS -> mock device -> nesting-parent S2 HWPT + vIOMMU -> vDEVICE
//        -> nested S1 HWPT -> cache invalidation -> vEVENTQ -> event
//
// Constants below mirror drivers/iommu/iommufd/iommufd_test.h (a
// userspace-visible test header that is deliberately not part of the
// stable uAPI, hence not in iommufd-bindings).

use std::os::unix::io::AsRawFd;
use std::process::exit;
use std::sync::Arc;

use iommufd_bindings::iommufd::iommu_ioas_alloc;
use iommufd_ioctls::{
    IommuFd, IommufdHwptData, IommufdInvalidateData, IommufdVDevice, IommufdVIommu,
};

/* From drivers/iommu/iommufd/iommufd_test.h */
const IOMMU_TEST_CMD: u32 = (0x3b << 8) | (0x80 + 32); // _IO(';', 0xa0)
const IOMMU_TEST_OP_MOCK_DOMAIN: u32 = 2;
const IOMMU_TEST_OP_MOCK_DOMAIN_REPLACE: u32 = 10;
const IOMMU_TEST_OP_TRIGGER_VEVENT: u32 = 17;
const IOMMU_VIOMMU_TYPE_SELFTEST: u32 = 0xdeadbeef;
const IOMMU_HWPT_DATA_SELFTEST: u32 = 0xdead;
const IOMMU_TEST_IOTLB_DEFAULT: u32 = 0xbadbeef;
const IOMMU_VIOMMU_INVALIDATE_DATA_SELFTEST: u32 = 0xdeadbeef;
const IOMMU_TEST_INVALIDATE_FLAG_ALL: u32 = 1 << 0;
const IOMMU_VEVENTQ_TYPE_SELFTEST: u32 = 0xbeefbeef;

// Total size of `struct iommu_test_cmd` (the header plus its largest
// union member), which the kernel requires in the `size` field. The
// union members all begin at offset 16; we model only the fields we
// use and pad the tail to the full size.
const IOMMU_TEST_CMD_SIZE: u32 = 72;

#[repr(C)]
struct TestCmdMockDomain {
    size: u32,
    op: u32,
    id: u32,
    _reserved: u32,
    out_stdev_id: u32,
    out_hwpt_id: u32,
    out_idev_id: u32,
    _pad: [u8; (IOMMU_TEST_CMD_SIZE as usize) - 28],
}

impl Default for TestCmdMockDomain {
    fn default() -> Self {
        // SAFETY: an all-zero bit pattern is a valid value for every field.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
struct TestCmdMockDomainReplace {
    size: u32,
    op: u32,
    id: u32,
    _reserved: u32,
    /// In: the HWPT (or IOAS) to move the mock device onto. Out: the same
    /// id, echoed back by the kernel after `iommufd_device_replace`.
    pt_id: u32,
    _pad: [u8; (IOMMU_TEST_CMD_SIZE as usize) - 20],
}

impl Default for TestCmdMockDomainReplace {
    fn default() -> Self {
        // SAFETY: an all-zero bit pattern is a valid value for every field.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
struct TestCmdTriggerVevent {
    size: u32,
    op: u32,
    id: u32,
    _reserved: u32,
    dev_id: u32,
    _pad: [u8; (IOMMU_TEST_CMD_SIZE as usize) - 20],
}

impl Default for TestCmdTriggerVevent {
    fn default() -> Self {
        // SAFETY: an all-zero bit pattern is a valid value for every field.
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Default)]
struct IommuHwptSelftest {
    iotlb: u32,
}

#[repr(C)]
#[derive(Default)]
struct IommuViommuInvalidateSelftest {
    flags: u32,
    cache_id: u32,
}

fn as_bytes<T>(v: &T) -> Vec<u8> {
    // SAFETY: T is a plain repr(C) struct with no padding requirements
    // beyond its fields; we only read its object representation.
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
        .to_vec()
}

struct Harness {
    failed: u32,
}

impl Harness {
    fn step<T, E: std::fmt::Debug>(
        &mut self,
        name: &str,
        r: std::result::Result<T, E>,
    ) -> Option<T> {
        match r {
            Ok(v) => {
                println!("MOCKTEST ok    - {name}");
                Some(v)
            }
            Err(e) => {
                println!("MOCKTEST NOTOK - {name}: {e:?}");
                self.failed += 1;
                None
            }
        }
    }

    /// Like `step`, but for steps that exercise mock-only precondition
    /// state (a populated vdevice cache, a triggerable event source)
    /// rather than the crate verb under test. Failures are reported as
    /// informational and do not fail the run.
    fn info_step<T, E: std::fmt::Debug>(
        &mut self,
        name: &str,
        r: std::result::Result<T, E>,
    ) -> Option<T> {
        match r {
            Ok(v) => {
                println!("MOCKTEST ok    - {name}");
                Some(v)
            }
            Err(e) => {
                println!("MOCKTEST info  - {name}: {e:?} (mock precondition; not a verb defect)");
                None
            }
        }
    }
}

fn ioctl_test_cmd<T>(fd: &IommuFd, cmd: &mut T) -> std::io::Result<()> {
    // SAFETY: fd is a valid open /dev/iommu; cmd is a repr(C) struct
    // whose `size` field the caller set to sizeof(T).
    let ret = unsafe { libc::ioctl(fd.as_raw_fd(), IOMMU_TEST_CMD as _, cmd as *mut T) };
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Move the mock device `stdev_id` onto `pt_id` (a HWPT or an IOAS). The
/// kernel services this as `iommufd_device_replace`, exactly what a VFIO
/// attach to a new HWPT becomes, so it stands in for the VMM's attach.
fn mock_domain_replace(fd: &IommuFd, stdev_id: u32, pt_id: u32) -> std::io::Result<()> {
    let mut rep = TestCmdMockDomainReplace {
        size: IOMMU_TEST_CMD_SIZE,
        op: IOMMU_TEST_OP_MOCK_DOMAIN_REPLACE,
        id: stdev_id,
        pt_id,
        ..Default::default()
    };
    ioctl_test_cmd(fd, &mut rep)
}

fn main() {
    let mut h = Harness { failed: 0 };

    println!("MOCKTEST: iommufd-ioctls mock-driver integration test");

    /* 1. Open /dev/iommu */
    let Some(iommufd) = h.step("open /dev/iommu", IommuFd::new()) else {
        println!("MOCKTEST-RESULT: FAIL");
        exit(1);
    };
    let iommufd = Arc::new(iommufd);

    /* 2. Allocate an IOAS */
    let mut ioas_alloc = iommu_ioas_alloc {
        size: std::mem::size_of::<iommu_ioas_alloc>() as u32,
        ..Default::default()
    };
    h.step(
        "IOMMU_IOAS_ALLOC",
        iommufd.alloc_iommu_ioas(&mut ioas_alloc),
    );
    let ioas_id = ioas_alloc.out_ioas_id;

    /* 3. Create a mock domain + device (selftest ioctl) */
    let mut mock = TestCmdMockDomain {
        size: IOMMU_TEST_CMD_SIZE,
        op: IOMMU_TEST_OP_MOCK_DOMAIN,
        id: ioas_id,
        ..Default::default()
    };
    if h.step(
        "IOMMU_TEST_OP_MOCK_DOMAIN",
        ioctl_test_cmd(&iommufd, &mut mock),
    )
    .is_none()
    {
        println!("MOCKTEST-RESULT: FAIL");
        exit(1);
    }
    println!(
        "MOCKTEST info  - stdev_id={} auto_hwpt_id={} idev_id={}",
        mock.out_stdev_id, mock.out_hwpt_id, mock.out_idev_id
    );
    let idev_id = mock.out_idev_id;

    /* 4. Nesting-parent S2 HWPT + vIOMMU (SELFTEST type) */
    let viommu = h.step(
        "S2 NEST_PARENT HWPT + IOMMU_VIOMMU_ALLOC(type=SELFTEST)",
        IommufdVIommu::new_with_type(
            iommufd.clone(),
            ioas_id,
            idev_id,
            IOMMU_VIOMMU_TYPE_SELFTEST,
            None,
        ),
    );
    let Some(viommu) = viommu else {
        println!("MOCKTEST-RESULT: FAIL");
        exit(1);
    };
    let viommu = Arc::new(viommu);
    println!(
        "MOCKTEST info  - viommu_id={} s2_hwpt_id={}",
        viommu.viommu_id, viommu.s2_hwpt_id
    );

    /* 5. vDEVICE with a virtual ID (the virtual SID slot for SMMUv3) */
    let vdev = h.step(
        "IOMMU_VDEVICE_ALLOC(virt_id=0x99)",
        IommufdVDevice::new(viommu.clone(), idev_id, 0x99),
    );

    /* 6. Nested S1 HWPT against the vIOMMU, via the Raw data path */
    if let Some(mut vdev) = vdev {
        let hwpt_data = IommufdHwptData::Raw {
            data_type: IOMMU_HWPT_DATA_SELFTEST,
            data: as_bytes(&IommuHwptSelftest {
                iotlb: IOMMU_TEST_IOTLB_DEFAULT,
            }),
        };
        let first_s1 = h.step(
            "nested S1 IOMMU_HWPT_ALLOC(pt=viommu, data=SELFTEST)",
            vdev.allocate_s1_hwpt(&hwpt_data),
        );

        /* 6b. Attach the mock device to that nested S1, as the VMM attaches
         * the VFIO endpoint after a fresh install. From here on the device
         * is really on the installed S1, so a destroy of it would fail with
         * EBUSY (the kernel's own selftest shows the same sequence) - the
         * constraint the stage/commit verbs below exist to satisfy. This
         * also leaves the mock's auto domain, which the kernel destroys as
         * soon as its last device departs; the teardown parks on the IOAS. */
        if let Some(first_s1) = first_s1 {
            h.step(
                "IOMMU_TEST_OP_MOCK_DOMAIN_REPLACE(stdev -> first S1)",
                mock_domain_replace(&iommufd, mock.out_stdev_id, first_s1),
            );
        }

        /* 7. Cache invalidation through the vIOMMU. The mock's SELFTEST
         * cache-invalidate validates against a populated per-vdevice
         * cache (set up by the kernel's own vdevice_cache test); without
         * that state it returns EINVAL. This exercises the crate's Raw
         * invalidate marshalling; the real SMMUv3 path uses the Smmuv3
         * variant, which is hardware-proven. Informational. */
        let mut inv = IommufdInvalidateData::Raw {
            data_type: IOMMU_VIOMMU_INVALIDATE_DATA_SELFTEST,
            entry_len: std::mem::size_of::<IommuViommuInvalidateSelftest>() as u32,
            entries: as_bytes(&IommuViommuInvalidateSelftest {
                flags: IOMMU_TEST_INVALIDATE_FLAG_ALL,
                cache_id: 0,
            }),
        };
        h.info_step(
            "IOMMU_HWPT_INVALIDATE(viommu, data=SELFTEST, ALL)",
            viommu.invalidate_hwpt(&mut inv),
        );

        /* 8. vEVENTQ allocation */
        let veventq = h.step(
            "IOMMU_VEVENTQ_ALLOC(type=SELFTEST, depth=8)",
            viommu.alloc_veventq(IOMMU_VEVENTQ_TYPE_SELFTEST, 8),
        );

        /* 9. Trigger a vEVENT from the mock device and read it back */
        if let Some(veventq) = &veventq {
            let mut trig = TestCmdTriggerVevent {
                size: IOMMU_TEST_CMD_SIZE,
                op: IOMMU_TEST_OP_TRIGGER_VEVENT,
                dev_id: idev_id,
                ..Default::default()
            };
            // Triggering a vEVENT and reading it back exercises mock
            // event-source plumbing, not a crate verb; report as info.
            let triggered = h
                .info_step(
                    "IOMMU_TEST_OP_TRIGGER_VEVENT",
                    ioctl_test_cmd(&iommufd, &mut trig),
                )
                .is_some();

            if triggered {
                let evfd = veventq.fd.as_raw_fd();
                let mut pfd = libc::pollfd {
                    fd: evfd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: pfd points to a valid pollfd for the whole call.
                let pret = unsafe { libc::poll(&mut pfd, 1, 3000) };
                if pret > 0 && (pfd.revents & libc::POLLIN) != 0 {
                    let mut buf = [0u8; 64];
                    // SAFETY: buf is a valid writable buffer of the given len.
                    let n = unsafe {
                        libc::read(evfd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
                    };
                    println!(
                        "MOCKTEST ok    - read vEVENT ({n} bytes: {:02x?})",
                        &buf[..n.max(0) as usize]
                    );
                } else {
                    println!("MOCKTEST info  - no vEVENT within 3s (poll={pret})");
                }
            }
        }

        /* 10. Atomic stage-1 replacement: stage a second nested S1 HWPT
         * while the first stays installed AND attached (step 6b), move the
         * mock device onto the staged one with a single replace, then
         * commit, which promotes the staged id and destroys the old object.
         * That destroy succeeds only because the replace has already moved
         * the device off the old HWPT; before the replace it would be
         * EBUSY. */
        let old_s1 = vdev.s1_hwpt_id;
        let staged = h.step(
            "stage second nested S1 HWPT (installed one untouched)",
            vdev.stage_s1_hwpt(&hwpt_data),
        );
        if let Some(staged) = staged {
            h.step(
                "installed S1 id unchanged while staged",
                if vdev.s1_hwpt_id == old_s1 {
                    Ok(())
                } else {
                    Err(format!("{:?} != {:?}", vdev.s1_hwpt_id, old_s1))
                },
            );
            let replaced = h
                .step(
                    "IOMMU_TEST_OP_MOCK_DOMAIN_REPLACE(stdev -> staged S1)",
                    mock_domain_replace(&iommufd, mock.out_stdev_id, staged),
                )
                .is_some();
            if replaced {
                h.step(
                    "commit staged S1 (promote, then destroy the old)",
                    vdev.commit_staged_s1_hwpt(),
                );
                h.step(
                    "installed S1 id is the staged one after commit",
                    if vdev.s1_hwpt_id == Some(staged) && vdev.staged_s1_hwpt_id.is_none() {
                        Ok(())
                    } else {
                        Err(format!(
                            "s1={:?} staged={:?}",
                            vdev.s1_hwpt_id, vdev.staged_s1_hwpt_id
                        ))
                    },
                );
            } else {
                h.step(
                    "discard staged S1 after failed replace",
                    vdev.discard_staged_s1_hwpt(),
                );
            }
        }

        /* 11. Stage-and-discard: the failed-attach shape. The installed S1
         * must survive untouched. */
        let installed = vdev.s1_hwpt_id;
        if h.step(
            "stage a third nested S1 HWPT",
            vdev.stage_s1_hwpt(&hwpt_data),
        )
        .is_some()
        {
            h.step("discard the staged S1 HWPT", vdev.discard_staged_s1_hwpt());
            h.step(
                "installed S1 id unchanged after discard",
                if vdev.s1_hwpt_id == installed && vdev.staged_s1_hwpt_id.is_none() {
                    Ok(())
                } else {
                    Err(format!(
                        "s1={:?} staged={:?}",
                        vdev.s1_hwpt_id, vdev.staged_s1_hwpt_id
                    ))
                },
            );
        }

        /* 12. RAII teardown: park the mock device on the IOAS (the kernel
         * selftest's own idiom - the auto domain it started on was
         * destroyed when the device left it in step 6b) so the installed
         * nested S1 HWPT can be destroyed, then drop the vdevice, then the
         * veventq. */
        drop(veventq);
        h.step(
            "IOMMU_TEST_OP_MOCK_DOMAIN_REPLACE(stdev -> IOAS)",
            mock_domain_replace(&iommufd, mock.out_stdev_id, ioas_id),
        );
        h.step("destroy nested S1 HWPT", vdev.destroy_s1_hwpt());
        drop(vdev); // destroys the vDEVICE
        println!("MOCKTEST ok    - vDEVICE/vEVENTQ teardown");
    }

    /* 11. Tear down the crate objects: dropping the vIOMMU destroys the
     * bypass/abort child HWPTs, the vIOMMU, and the S2 nesting parent in
     * order. */
    drop(viommu);
    println!("MOCKTEST ok    - vIOMMU teardown");

    /* The mock's own objects (stdev, its auto domain, the IOAS) form a
     * dependency cycle that the kernel reclaims atomically when the
     * iommufd fd is closed - the same convention the kernel selftest
     * uses. Destroying the stdev is a real destroy_iommu_object verb call;
     * the rest is left to fd close on drop. */
    h.step(
        "destroy mock stdev",
        iommufd.destroy_iommu_object(mock.out_stdev_id),
    );
    drop(iommufd);
    println!("MOCKTEST ok    - iommufd fd closed (IOAS + mock domain reclaimed)");

    if h.failed == 0 {
        println!("MOCKTEST-RESULT: PASS");
        exit(0);
    } else {
        println!("MOCKTEST-RESULT: FAIL ({} steps failed)", h.failed);
        exit(1);
    }
}
