# mock-tests

An end-to-end exerciser for the `iommufd-ioctls` vIOMMU / vDEVICE / nested
HWPT / vEVENTQ verbs, run against the kernel's own iommufd selftest mock
driver (`CONFIG_IOMMUFD_TEST=y`). It needs no IOMMU, no SMMU and no
passed-through device, so it runs anywhere a suitable kernel boots.

It is a plain binary rather than a `#[test]`, and `publish = false`: it must
run as root against a live kernel, so `cargo test` neither builds it into the
harness nor should ever launch it. Workspace CI compiles it (via
`--all-targets`) and stops there.

## What it covers

The full accelerated-vSMMUv3 allocation flow, in the order a VMM performs it:

    IOAS -> mock device -> nesting-parent S2 HWPT + vIOMMU -> vDEVICE
         -> nested S1 HWPT -> cache invalidation -> vEVENTQ -> event

and, on top of that, the stage/replace/commit sequence the staged-S1 verbs
exist for:

- attach the device to a first nested S1 HWPT,
- `stage_s1_hwpt` a second one while the first stays attached,
- `MOCK_DOMAIN_REPLACE(stdev -> staged S1)`, the kernel's own replace,
- `commit_staged_s1_hwpt`, which promotes and then **really destroys** the
  old HWPT,
- and the `discard_staged_s1_hwpt` path.

That real destroy-of-old is what this leg adds over the crate's unit tests.
Those use a `/dev/null`-backed fd, so they can prove ordering (a guard that
fires before its ioctl; a promotion that happens even when the destroy
fails) but nothing about what the kernel does to the objects afterwards.

## Running it

`IOMMUFD_TEST` depends on `DEBUG_KERNEL`, `FAULT_INJECTION` and
`RUNTIME_TESTING_MENU`, none of which a fleet host carries in its host
kernel, so run it in a **guest**. Build a kernel with
`FAULT_INJECTION`, `FAULT_INJECTION_DEBUG_FS`, `IOMMUFD_TEST` and `DEBUG_FS`,
build this binary for a musl target so it needs no libc in the initramfs, and
boot the two together:

    cloud-hypervisor --kernel Image --initramfs initrd.cpio.gz \
      --cmdline "console=ttyAMA0 earlycon rdinit=/init panic=-1" \
      --cpus boot=2 --memory size=1G --serial file=serial.log --console off -v
    grep -a MOCKTEST-RESULT serial.log

The vsmmuv3-testing-kit's `tools/mk-mock-initrd.sh` builds that initramfs and
carries the kernel-config recipe in its header.

Output is one `MOCKTEST ok|NOTOK|info <label>` line per step and a final
`MOCKTEST-RESULT: PASS|FAIL (<n> steps failed)`. Gate on the
`MOCKTEST-RESULT` line, not on a line count: a PASS at
`vsmmuv3-verbs-mainline-validated-9` was 24 `MOCKTEST ok` lines on the box
whose serial capture was complete, and serial output is exactly the thing a
run can lose the middle of.

## The trap this harness had, and the kernel confirmed

The auto domain a mock device starts on (`out_hwpt_id` from `MOCK_DOMAIN`) is
**destroyed by the kernel** the moment the device is replaced off it
(`iommufd_hw_pagetable_put` -> `auto_domain` -> destroy). Parking the device
back onto that id at teardown returns `ENOENT`, the S1 destroy then `EBUSY`s,
and the vIOMMU teardown cascades from there. Park on the **IOAS** id instead,
which is the kernel selftest's own idiom. Two independent reviews predicted
this failure, and running the pre-fix harness reproduced it exactly.

The other half: a harness that never *attaches* the device to the first
nested HWPT proves nothing about the EBUSY-while-attached constraint these
verbs exist for. Attach first, then stage, replace and commit.

Status: PASS on two aarch64 boxes at tag
`vsmmuv3-verbs-mainline-validated-9`.
