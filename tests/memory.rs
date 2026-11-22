use virtual_daisy::memory::SystemMemory;

#[test]
fn repeated_policy_lookups_follow_live_cache_and_mpu_changes() {
    let mut memory = SystemMemory::default();
    let address = 0x2400_0000;
    memory.write_dma(address, 4, 1).unwrap();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    for _ in 0..20 {
        assert_eq!(memory.read_cpu(address, 4).unwrap(), 1);
    }
    memory.write_dma(address, 4, 2).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 1);
    memory.write_scb(0xd14, 0).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 2);
    memory.write_scb(0xd14, 1 << 16).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 1);
    memory.write_scb(0xd94, 1).unwrap();
    assert!(memory.read_cpu(address, 4).is_err());
    memory.write_scb(0xd9c, address).unwrap();
    memory.write_scb(0xda0, 0x030b0000 | (4 << 1) | 1).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 1);
    memory.write_scb(0xda8, 0x03080000 | (4 << 1) | 1).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 2);
    memory.reset_registers();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 2);
}

#[test]
fn warmed_data_policy_does_not_bypass_execute_never_or_late_mpu_updates() {
    let mut memory = SystemMemory::default();
    let address = 0x2400_0000;
    memory.write_dma(address, 4, 0x4770bf00).unwrap();
    memory.write_scb(0xd14, (1 << 16) | (1 << 17)).unwrap();
    memory.write_scb(0xd9c, address).unwrap();
    let attrs = 0x030b0000 | (4 << 1) | 1;
    memory.write_scb(0xda0, attrs | (1 << 28)).unwrap();
    memory.write_scb(0xd94, 5).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 0x4770bf00);
    assert!(memory.decode_instruction(address, 2).is_err());
    memory.write_scb(0xda0, attrs).unwrap();
    assert_eq!(memory.decode_instruction(address, 2).unwrap(), 0xbf00);
    memory.write_scb(0xda0, attrs | (1 << 28)).unwrap();
    assert!(memory.decode_instruction(address, 2).is_err());
    memory.write_scb(0xd94, 0).unwrap();
    assert_eq!(memory.decode_instruction(address, 2).unwrap(), 0xbf00);
}

#[test]
fn speculative_reads_preserve_misses_and_dma_cache_coherence() {
    let mut memory = SystemMemory::default();
    let address = 0x24001000;
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_dma(address, 4, 1).unwrap();
    assert_eq!(memory.peek_cpu32(address), None);
    assert!(!memory.can_write_cached32(address));
    memory.write_dma(address, 4, 2).unwrap();
    assert_eq!(memory.read_cpu(address, 4).unwrap(), 2);
    assert_eq!(memory.peek_cpu32(address), Some(2));
    assert!(memory.can_write_cached32(address));
    memory.write_dma(address, 4, 3).unwrap();
    assert_eq!(memory.peek_cpu32(address), Some(2));
    memory.write_cpu(address, 4, 4).unwrap();
    assert_eq!(memory.read_dma(address, 4).unwrap(), 3);
    memory.write_scb(0xf68, address).unwrap();
    assert_eq!(memory.read_dma(address, 4).unwrap(), 4);
    memory.write_scb(0xf5c, address).unwrap();
    assert_eq!(memory.peek_cpu32(address), None);
    assert_eq!(memory.peek_cpu32(0x40000000), None);
    assert_eq!(memory.peek_cpu32(0x90000000), None);
    assert_eq!(memory.peek_cpu32(address + 1), None);
}

#[test]
fn grouped_transfers_match_byte_accesses_across_cache_and_page_boundaries() {
    for attributes in [
        0x0308_0000,
        0x0302_0000,
        0x030b_0000,
        0x0303_0000,
        0x030f_0000,
    ] {
        let mut grouped = SystemMemory::default();
        let mut bytes = SystemMemory::default();
        for memory in [&mut grouped, &mut bytes] {
            memory.write_scb(0xd14, 1 << 16).unwrap();
            memory.write_scb(0xd9c, 0x2400_0000).unwrap();
            memory.write_scb(0xda0, attributes | (18 << 1) | 1).unwrap();
            memory.write_scb(0xd94, 5).unwrap();
        }
        let mut seed = 0x12345678u32;
        for offset in [0, 1, 27, 28, 29, 30, 31, 32, 4093, 4094, 4095, 4096, 8191] {
            for width in [1, 2, 4] {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let address = 0x24000000 + offset;
                grouped.write_cpu(address, width, seed).unwrap();
                for byte in 0..width {
                    bytes
                        .write_cpu(address + byte as u32, 1, seed >> (byte * 8) & 255)
                        .unwrap();
                }
                let expected = (0..width).fold(0, |v, byte| {
                    v | bytes.read_cpu(address + byte as u32, 1).unwrap() << (byte * 8)
                });
                assert_eq!(grouped.read_cpu(address, width).unwrap(), expected);
                assert_eq!(
                    grouped.read_dma(address, width).unwrap(),
                    bytes.read_dma(address, width).unwrap()
                );
            }
        }
        for offset in (0..8256).step_by(32) {
            for memory in [&mut grouped, &mut bytes] {
                memory.write_scb(0xf68, 0x24000000 + offset).unwrap();
            }
        }
        for offset in 0..8256 {
            assert_eq!(
                grouped.read_dma(0x24000000 + offset, 1).unwrap(),
                bytes.read_dma(0x24000000 + offset, 1).unwrap()
            );
        }
    }
}

#[test]
fn cpu_write_back_data_is_invisible_to_dma_until_cleaned() {
    let mut memory = SystemMemory::default();
    memory.write_dma(0x2400_0000, 4, 0x11223344).unwrap();
    memory.write_scb(0xd14, 1 << 16).unwrap(); // CCR.DC.
    assert_eq!(memory.read_cpu(0x2400_0000, 4).unwrap(), 0x11223344);
    memory.write_cpu(0x2400_0000, 4, 0x55667788).unwrap();
    assert_eq!(memory.read_cpu(0x2400_0000, 4).unwrap(), 0x55667788);
    assert_eq!(memory.read_dma(0x2400_0000, 4).unwrap(), 0x11223344);
    memory.write_scb(0xf68, 0x2400_0000).unwrap(); // DCCMVAC.
    assert_eq!(memory.read_dma(0x2400_0000, 4).unwrap(), 0x55667788);
}

#[test]
fn dma_receive_is_stale_in_cpu_cache_until_invalidation() {
    let mut memory = SystemMemory::default();
    memory.write_dma(0x3000_0000, 4, 0x11111111).unwrap();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    assert_eq!(memory.read_cpu(0x3000_0000, 4).unwrap(), 0x11111111);
    memory.write_dma(0x3000_0000, 4, 0x22222222).unwrap();
    assert_eq!(memory.read_cpu(0x3000_0000, 4).unwrap(), 0x11111111);
    memory.write_scb(0xf5c, 0x3000_0000).unwrap();
    assert_eq!(memory.read_cpu(0x3000_0000, 4).unwrap(), 0x22222222);
}

#[test]
fn firmware_noncacheable_mpu_region_keeps_dma_buffers_coherent() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd98, 0).unwrap();
    memory.write_scb(0xd9c, 0x3000_0000).unwrap();
    memory.write_scb(0xda0, 0x030c001d).unwrap(); // Normal, shared, noncached, 32KiB.
    memory.write_scb(0xd94, 5).unwrap();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x3000_0000, 4, 0x12345678).unwrap();
    assert_eq!(memory.read_dma(0x3000_0000, 4).unwrap(), 0x12345678);
    memory.write_dma(0x3000_0000, 4, 0x87654321).unwrap();
    assert_eq!(memory.read_cpu(0x3000_0000, 4).unwrap(), 0x87654321);
}

#[test]
fn cmsis_can_enumerate_and_clean_all_data_cache_sets_and_ways() {
    let mut memory = SystemMemory::default();
    assert_eq!(memory.read_scb(0xd78).unwrap(), 0x09000003);
    assert_eq!(memory.read_scb(0xd7c).unwrap(), 0x8303c003);
    assert_eq!(memory.read_scb(0xd80).unwrap(), 0xf00fe019); // 16KiB, 4 ways, 32B.
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x2400_0000, 4, 0x12345678).unwrap();
    for set in 0..128 {
        for way in 0..4 {
            memory.write_scb(0xf74, (set << 5) | (way << 30)).unwrap();
        }
    }
    assert_eq!(memory.read_dma(0x2400_0000, 4).unwrap(), 0x12345678);
}

#[test]
fn dma_access_is_independent_of_cpu_mpu_permissions() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_scb(0xd94, 1).unwrap(); // No enabled regions or background map.
    assert!(memory.read_cpu(0x2400_0000, 4).is_err());
    memory.write_dma(0x2400_0000, 4, 0x12345678).unwrap();
    assert_eq!(memory.read_dma(0x2400_0000, 4).unwrap(), 0x12345678);
}

#[test]
fn invalidating_a_line_can_discard_dirty_neighboring_data() {
    let mut memory = SystemMemory::default();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x24000000, 4, 0x1234).unwrap();
    memory.write_dma(0x24000010, 4, 0x5678).unwrap();
    memory.write_scb(0xf5c, 0x24000010).unwrap();
    assert_eq!(memory.read_cpu(0x24000000, 4).unwrap(), 0);
    assert_eq!(memory.read_cpu(0x24000010, 4).unwrap(), 0x5678);
}

#[test]
fn mpu_subregion_holes_fall_through_to_a_lower_priority_region() {
    let mut memory = SystemMemory::default();
    for (region, attrs) in [
        (0, 0x03080000 | (18 << 1) | 1),
        (1, 0x030b0000 | (1 << 8) | (7 << 1) | 1),
    ] {
        memory.write_scb(0xd98, region).unwrap();
        memory.write_scb(0xd9c, 0x24000000).unwrap();
        memory.write_scb(0xda0, attrs).unwrap();
    }
    memory.write_scb(0xd94, 5).unwrap();
    memory.write_scb(0xd14, 1 << 16).unwrap();
    memory.write_cpu(0x24000000, 4, 0x1111).unwrap(); // Disabled subregion -> NC region0.
    memory.write_cpu(0x24000020, 4, 0x2222).unwrap(); // Higher priority WBWA region1.
    assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 0x1111);
    assert_eq!(memory.read_dma(0x24000020, 4).unwrap(), 0);
}

#[test]
fn write_through_and_write_back_without_allocation_keep_their_documented_policies() {
    for (cb, write_hit_visible) in [(2, true), (3, false)] {
        let mut memory = SystemMemory::default();
        memory.write_scb(0xd9c, 0x24000000).unwrap();
        memory
            .write_scb(0xda0, 0x03000000 | (cb << 16) | (18 << 1) | 1)
            .unwrap();
        memory.write_scb(0xd94, 5).unwrap();
        memory.write_scb(0xd14, 1 << 16).unwrap();
        memory.write_cpu(0x24000000, 4, 1).unwrap(); // Miss: no allocation, reaches RAM.
        assert_eq!(memory.read_dma(0x24000000, 4).unwrap(), 1);
        assert_eq!(memory.read_cpu(0x24000000, 4).unwrap(), 1); // Read allocates.
        memory.write_cpu(0x24000000, 4, 2).unwrap();
        assert_eq!(
            memory.read_dma(0x24000000, 4).unwrap(),
            if write_hit_visible { 2 } else { 1 }
        );
    }
}
