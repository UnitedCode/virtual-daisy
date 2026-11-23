// Generated from independent QEMU records by cpu/examples/qemu_isa.rs.
pub const CPU_DIVIDE_CASES: [[u32; 6]; 4] = [
    [
        0xfb91f0f2, 0x80000000, 0xffffffff, 0xa80a0000, 0x80000000, 0xa80a0000,
    ],
    [
        0xfb91f0f2, 0xfffffff9, 0x00000003, 0xa80a0000, 0xfffffffe, 0xa80a0000,
    ],
    [
        0xfbb1f0f2, 0xffffffff, 0x00000003, 0xa80a0000, 0x55555555, 0xa80a0000,
    ],
    [
        0xfbb1f0f2, 0x12345678, 0x00000000, 0xa80a0000, 0x00000000, 0xa80a0000,
    ],
];
