pub(super) fn unsupported_opcode_message(opcode: u8) -> String {
    let detail = match opcode {
        0x09
        | 0x0a
        | 0x1e..=0x21
        | 0x2f
        | 0x37
        | 0x3f..=0x42
        | 0x50
        | 0x61
        | 0x65
        | 0x69
        | 0x6d
        | 0x71
        | 0x75
        | 0x79
        | 0x7b
        | 0x7d
        | 0x7f
        | 0x81
        | 0x83
        | 0x85
        | 0x88..=0x8a
        | 0x94
        | 0xad => Some(("long primitive bytecode", "primitive-completeness")),
        0x0b..=0x0d
        | 0x17
        | 0x22..=0x25
        | 0x30
        | 0x38
        | 0x43..=0x46
        | 0x51
        | 0x62
        | 0x66
        | 0x6a
        | 0x6e
        | 0x72
        | 0x76
        | 0x86
        | 0x8b..=0x8d
        | 0x95
        | 0x96
        | 0xae => Some(("float primitive bytecode", "primitive-completeness")),
        0x0e
        | 0x0f
        | 0x18
        | 0x26..=0x29
        | 0x31
        | 0x39
        | 0x47..=0x4a
        | 0x52
        | 0x63
        | 0x67
        | 0x6b
        | 0x6f
        | 0x73
        | 0x77
        | 0x87
        | 0x8e..=0x90
        | 0x97
        | 0x98
        | 0xaf => Some(("double primitive bytecode", "primitive-completeness")),
        0x14 => Some((
            "long/float/double constant loading",
            "primitive-completeness",
        )),
        0x5a..=0x5f => Some(("full stack manipulation opcodes", "primitive-completeness")),
        0x78 | 0x7a | 0x7c | 0x7e | 0x80 | 0x82 => {
            Some(("int bitwise and shift opcodes", "primitive-completeness"))
        }
        0xaa | 0xab => Some(("switch bytecodes", "primitive-completeness")),
        0xbb..=0xbf => Some(("runtime allocation", "runtime-allocation")),
        0xc1 => Some(("instanceof", "primitive-completeness")),
        0xc2 | 0xc3 => Some(("monitors/synchronization", "concurrency-profile")),
        0xc4 => Some(("wide local-variable bytecodes", "primitive-completeness")),
        0xc5 => Some(("multidimensional arrays", "primitive-completeness")),
        _ => None,
    };

    if let Some((feature, milestone)) = detail {
        format!(
            "fvm-aot unsupported opcode 0x{opcode:02x}; required feature: {feature}; planned milestone: {milestone}"
        )
    } else {
        format!(
            "fvm-aot unsupported opcode 0x{opcode:02x}; supported subset includes int-compatible locals/arithmetic/branches, same-class objects/static helpers/fields, arrays, core String/Object intrinsics, println, and Http.respond"
        )
    }
}
