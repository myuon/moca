// Moca Standard Library - Prelude
// This file is automatically loaded when running Moca programs.

// ============================================================================
// Syscall Numbers (internal use)
// ============================================================================
// Syscall 1: write(fd, buf, count) -> bytes_written
// Syscall 2: open(path, flags) -> fd
// Syscall 3: close(fd) -> status
// Syscall 4: read(fd, count) -> string
// Syscall 5: socket(domain, type) -> fd
// Syscall 6: connect(fd, host, port) -> status
// Syscall 7: bind(fd, host, port) -> status
// Syscall 8: listen(fd, backlog) -> status
// Syscall 9: accept(fd) -> client_fd
// Syscall 10: time() -> epoch_seconds
// Syscall 11: time_nanos() -> epoch_nanoseconds

// ============================================================================
// POSIX-like Constants (as functions to avoid polluting the stack)
// ============================================================================

// File open flags (Linux-compatible values)
fun O_RDONLY() -> int { return 0; }    // Read only
fun O_WRONLY() -> int { return 1; }    // Write only
fun O_CREAT() -> int { return 64; }    // Create file if not exists
fun O_TRUNC() -> int { return 512; }   // Truncate existing file

// Socket constants (Linux-compatible values)
fun AF_INET() -> int { return 2; }     // IPv4 address family
fun SOCK_STREAM() -> int { return 1; } // TCP socket type

// Error codes (negative return values)
fun EBADF() -> int { return -1; }           // Bad file descriptor
fun ENOENT() -> int { return -2; }          // No such file or directory
fun EACCES() -> int { return -3; }          // Permission denied
fun ECONNREFUSED() -> int { return -4; }    // Connection refused
fun ETIMEDOUT() -> int { return -5; }       // Connection timed out
fun EAFNOSUPPORT() -> int { return -6; }    // Address family not supported
fun ESOCKTNOSUPPORT() -> int { return -7; } // Socket type not supported
fun EADDRINUSE() -> int { return -8; }      // Address already in use

// ============================================================================
// Low-level I/O Functions (using __syscall)
// ============================================================================

// Open a file and return a file descriptor.
// flags: O_RDONLY(), O_WRONLY(), O_CREAT(), O_TRUNC() (can be combined with |)
// Returns: fd (>=3) on success, negative error code on failure
fun open(path: string, flags: int) -> int {
    return __syscall(2, path, flags);
}

// Write to a file descriptor.
// fd: 1 = stdout, 2 = stderr, >=3 = file/socket
// Returns: bytes written on success, negative error code on failure
fun write(fd: int, buf: string, count: int) -> int {
    return __syscall(1, fd, buf, count);
}

// Read from a file descriptor.
// Returns: string on success, or throws on error
fun read(fd: int, count: int) -> string {
    return __syscall(4, fd, count);
}

// Close a file descriptor.
// Returns: 0 on success, negative error code on failure
fun close(fd: int) -> int {
    return __syscall(3, fd);
}

// Create a socket.
// domain: AF_INET() (2) for IPv4
// typ: SOCK_STREAM() (1) for TCP
// Returns: socket fd on success, negative error code on failure
fun socket(domain: int, typ: int) -> int {
    return __syscall(5, domain, typ);
}

// Connect a socket to a remote host.
// Returns: 0 on success, negative error code on failure
fun connect(fd: int, host: string, port: int) -> int {
    return __syscall(6, fd, host, port);
}

// Bind a socket to a local address.
// host: "0.0.0.0" for all interfaces, "127.0.0.1" for localhost only
// Returns: 0 on success, negative error code on failure
fun bind(fd: int, host: string, port: int) -> int {
    return __syscall(7, fd, host, port);
}

// Listen for incoming connections on a bound socket.
// backlog: maximum number of pending connections (ignored in current implementation)
// Returns: 0 on success, negative error code on failure
fun listen(fd: int, backlog: int) -> int {
    return __syscall(8, fd, backlog);
}

// Accept an incoming connection on a listening socket.
// Returns: new socket fd for the client connection, or negative error code on failure
fun accept(fd: int) -> int {
    return __syscall(9, fd);
}

// ============================================================================
// Time Functions
// ============================================================================

// Get current time as Unix epoch seconds.
fun time() -> int {
    return __syscall(10);
}

// Get current time as Unix epoch nanoseconds.
fun time_nanos() -> int {
    return __syscall(11);
}

// ============================================================================
// Value to String Conversion — Helpers
// ============================================================================

// Count decimal digits of an integer (no heap allocation).
@inline
fun _int_digit_count(n: int) -> int {
    if n == 0 {
        return 1;
    }
    let count = 0;
    let val = n;
    if val < 0 {
        count = 1;
        val = -(val / 10);
        count = count + 1;
        // val is now positive (works even for i64::MIN)
    }
    while val > 0 {
        val = val / 10;
        count = count + 1;
    }
    return count;
}

// Write integer digits into buf at offset, return new offset (no heap allocation).
@inline
fun _int_write_to(buf: ptr<int>, off: int, n: int) -> int {
    if n == 0 {
        buf[off] = 48;
        return off + 1;
    }
    let dcount = _int_digit_count(n);
    let val = n;
    if val < 0 {
        buf[off] = 45;
        // Extract last digit safely: -(MIN % 10) won't overflow
        let pos = off + dcount - 1;
        buf[pos] = -(val % 10) + 48;
        val = -(val / 10);
        pos = pos - 1;
        while val > 0 {
            buf[pos] = val % 10 + 48;
            val = val / 10;
            pos = pos - 1;
        }
    } else {
        let pos = off + dcount - 1;
        while val > 0 {
            buf[pos] = val % 10 + 48;
            val = val / 10;
            pos = pos - 1;
        }
    }
    return off + dcount;
}

// Copy string data into buf at offset, return new offset.
@inline
fun _str_copy_to(buf: ptr<int>, off: int, s: string) -> int {
    let sptr = s.data;
    let slen = s.len;
    let j = 0;
    while j < slen {
        buf[off + j] = sptr[j];
        j = j + 1;
    }
    return off + slen;
}

// Return string length of a bool ("true"=4, "false"=5).
@inline
fun _bool_str_len(b: bool) -> int {
    if b {
        return 4;
    }
    return 5;
}

// Write "true" or "false" into buf at offset, return new offset.
@inline
fun _bool_write_to(buf: ptr<int>, off: int, b: bool) -> int {
    if b {
        buf[off] = 116;
        buf[off + 1] = 114;
        buf[off + 2] = 117;
        buf[off + 3] = 101;
        return off + 4;
    }
    buf[off] = 102;
    buf[off + 1] = 97;
    buf[off + 2] = 108;
    buf[off + 3] = 115;
    buf[off + 4] = 101;
    return off + 5;
}

// ============================================================================
// ============================================================================
// Ryu Float-to-String Algorithm
// ============================================================================

fun __float_bits(f: float) -> int {
    return asm(f) -> i64 { __emit("F64ReinterpretAsI64"); };
}
fun _ushr(a: int, b: int) -> int {
    return asm(a, b) -> i64 { __emit("I64ShrU"); };
}

fun _ryu_init_tables() -> array<int> {
    return [
        // POW5_TABLE[0..25]
        1, 5, 25, 125, 625, 3125, 15625, 78125, 390625, 1953125,
        9765625, 48828125, 244140625, 1220703125, 6103515625,
        30517578125, 152587890625, 762939453125, 3814697265625,
        19073486328125, 95367431640625, 476837158203125, 2384185791015625,
        11920928955078125, 59604644775390625, 298023223876953125,
        // DOUBLE_POW5_SPLIT2[26..51]: 13 pairs (lo, hi)
        0, 1152921504606846976,
        0, 1490116119384765625,
        1032610780636961552, 1925929944387235853,
        7910200175544436838, 1244603055572228341,
        -1504838264676837686, 1608611746708759036,
        -5421850118411349444, 2079081953128979843,
        6607496772837067824, 1343575221513417750,
        -1113817083813899013, 1736530273035216783,
        -5409364890226003632, 2244412773384604712,
        1605989338741628675, 1450417759929778918,
        -8816519005292960336, 1874621017369538693,
        665883850346957067, 1211445438634777304,
        -3514853404985837908, 1565756531257009982,
        // DOUBLE_POW5_INV_SPLIT2[52..81]: 15 pairs (lo, hi)
        1, 2305843009213693952,
        5955668970331000884, 1784059615882449851,
        8982663654677661702, 1380349269358112757,
        7286864317269821294, 2135987035920910082,
        7005857020398200553, 1652639921975621497,
        -481418970354774919, 1278668206209430417,
        8928596168509315048, 1978643211784836272,
        -8371072500651252758, 1530901034580419511,
        597001226353042382, 1184477304306571148,
        1527430471115325346, 1832889850782397517,
        -5913534206540532074, 1418129833677084982,
        5577825024675947042, 2194449627517475473,
        -7439769533505684065, 1697873161311732311,
        -8133250842069730034, 1313665730009899186,
        -5745727253942878843, 2032799256770390445,
        // POW5_OFFSETS[82..102]
        0, 0, 0, 0,
        1073741824, 1500076437, 1431590229, 1448432917,
        1091896580, 1079333904, 1146442053, 1146111296,
        1163220304, 1073758208, 2521039936, 1431721317,
        1413824581, 1075134801, 1431671125, 1363170645,
        261,
        // POW5_INV_OFFSETS[103..121]
        1414808916, 67458373, 268701696, 4195348,
        1073807360, 1091917141, 1108, 65604,
        1073741824, 1140850753, 1346716752, 1431634004,
        1365595476, 1073758208, 16777217, 66816,
        1364284433, 89478484, 0
    ];
}

fun _ryu_log10_pow2(e: int) -> int { return _ushr(e * 78913, 18); }
fun _ryu_log10_pow5(e: int) -> int { return _ushr(e * 732923, 20); }
fun _ryu_pow5bits(e: int) -> int { return _ushr(e * 1217359, 19) + 1; }

fun _ryu_multiple_of_pow5(v: int, p: int) -> bool {
    let val = v;
    let i = 0;
    while i < p {
        if val % 5 != 0 { return false; }
        val = val / 5;
        i = i + 1;
    }
    return true;
}

fun _ryu_multiple_of_pow2(v: int, p: int) -> bool {
    return (v & ((1 << p) - 1)) == 0;
}

fun _ryu_u64_lt(a: int, b: int) -> bool {
    let sa = _ushr(a, 63);
    let sb = _ushr(b, 63);
    if sa != sb { return sa < sb; }
    return a < b;
}

fun _ryu_compute_pow5(tbl: array<int>, i: int, out: array<int>) {
    let base = i / 26;
    let base2 = base * 26;
    let offset = i - base2;
    let mul_lo = tbl[26 + base * 2];
    let mul_hi = tbl[26 + base * 2 + 1];
    if offset == 0 {
        out[0] = mul_lo;
        out[1] = mul_hi;
        return;
    }
    let m = tbl[offset];
    let b0_lo = m * mul_lo;
    let b0_hi = __umul128_hi(m, mul_lo);
    let b2_lo = m * mul_hi;
    let b2_hi = __umul128_hi(m, mul_hi);
    let delta = _ryu_pow5bits(i) - _ryu_pow5bits(base2);
    let s = 64 - delta;
    let correction = _ushr(tbl[82 + i / 16], (i % 16) * 2) & 3;
    let lo = _ushr(b0_lo, delta) | (b0_hi << s);
    let hi = _ushr(b0_hi, delta);
    let mid = b2_lo << s;
    let sum_lo = lo + mid;
    let carry = 0;
    if _ryu_u64_lt(sum_lo, lo) { carry = 1; }
    let sum_lo2 = sum_lo + correction;
    if _ryu_u64_lt(sum_lo2, sum_lo) { carry = carry + 1; }
    let sum_hi = hi + (_ushr(b2_lo, delta) | (b2_hi << s)) + carry;
    out[0] = sum_lo2;
    out[1] = sum_hi;
}

fun _ryu_compute_inv_pow5(tbl: array<int>, i: int, out: array<int>) {
    let base = (i + 25) / 26;
    let base2 = base * 26;
    let offset = base2 - i;
    let mul_lo = tbl[52 + base * 2];
    let mul_hi = tbl[52 + base * 2 + 1];
    if offset == 0 {
        out[0] = mul_lo;
        out[1] = mul_hi;
        return;
    }
    let m = tbl[offset];
    let b0_lo = m * (mul_lo - 1);
    let b0_hi = __umul128_hi(m, mul_lo - 1);
    let b2_lo = m * mul_hi;
    let b2_hi = __umul128_hi(m, mul_hi);
    let delta = _ryu_pow5bits(base2) - _ryu_pow5bits(i);
    let s = 64 - delta;
    let correction = 1 + (_ushr(tbl[103 + i / 16], (i % 16) * 2) & 3);
    let lo = _ushr(b0_lo, delta) | (b0_hi << s);
    let hi = _ushr(b0_hi, delta);
    let mid = b2_lo << s;
    let sum_lo = lo + mid;
    let carry = 0;
    if _ryu_u64_lt(sum_lo, lo) { carry = 1; }
    let sum_lo2 = sum_lo + correction;
    if _ryu_u64_lt(sum_lo2, sum_lo) { carry = carry + 1; }
    let sum_hi = hi + (_ushr(b2_lo, delta) | (b2_hi << s)) + carry;
    out[0] = sum_lo2;
    out[1] = sum_hi;
}


fun _ryu_mul_shift_64(m: int, mul_lo: int, mul_hi: int, j: int) -> int {
    let b0_hi = __umul128_hi(m, mul_lo);
    let b2 = m * mul_hi;
    let b2_hi = __umul128_hi(m, mul_hi);
    let sum = b0_hi + b2;
    if _ryu_u64_lt(sum, b0_hi) { b2_hi = b2_hi + 1; }
    let shift = j - 64;
    return _ushr(sum, shift) | (b2_hi << (64 - shift));
}





fun _ryu_decimal_length17(v: int) -> int {
    if v >= 10000000000000000 { return 17; }
    if v >= 1000000000000000 { return 16; }
    if v >= 100000000000000 { return 15; }
    if v >= 10000000000000 { return 14; }
    if v >= 1000000000000 { return 13; }
    if v >= 100000000000 { return 12; }
    if v >= 10000000000 { return 11; }
    if v >= 1000000000 { return 10; }
    if v >= 100000000 { return 9; }
    if v >= 10000000 { return 8; }
    if v >= 1000000 { return 7; }
    if v >= 100000 { return 6; }
    if v >= 10000 { return 5; }
    if v >= 1000 { return 4; }
    if v >= 100 { return 3; }
    if v >= 10 { return 2; }
    return 1;
}

fun _ryu_d2d(ieee_mantissa: int, ieee_exponent: int, out: array<int>) {
    let e2: int = 0;
    let m2: int = 0;
    if ieee_exponent == 0 {
        e2 = 1 - 1023 - 52 - 2;
        m2 = ieee_mantissa;
    } else {
        e2 = ieee_exponent - 1023 - 52 - 2;
        m2 = (1 << 52) | ieee_mantissa;
    }
    let even = (m2 & 1) == 0;
    let accept_bounds = even;
    let mv = 4 * m2;
    let mm_shift = 0;
    if ieee_mantissa != 0 || ieee_exponent <= 1 { mm_shift = 1; }
    let mp = mv + 2;
    let mm = mv - 1 - mm_shift;
    let vr: int = 0;
    let vp: int = 0;
    let vm: int = 0;
    let e10: int = 0;
    let vm_is_trailing_zeros = false;
    let vr_is_trailing_zeros = false;
    let last_removed_digit = 0;
    let tbl = _ryu_init_tables();
    let scratch: array<int> = [0, 0];
    if e2 >= 0 {
        let q = _ryu_log10_pow2(e2);
        if e2 > 3 { q = q - 1; }
        e10 = q;
        let k = 125 + _ryu_pow5bits(q) - 1;
        let i = 0 - e2 + q + k;
        _ryu_compute_inv_pow5(tbl, q, scratch);
        let mul_lo = scratch[0];
        let mul_hi = scratch[1];
        vr = _ryu_mul_shift_64(mv, mul_lo, mul_hi, i);
        vp = _ryu_mul_shift_64(mp, mul_lo, mul_hi, i);
        vm = _ryu_mul_shift_64(mm, mul_lo, mul_hi, i);
        if q <= 21 {
            if mv % 5 == 0 {
                vr_is_trailing_zeros = _ryu_multiple_of_pow5(mv, q);
            } else if accept_bounds {
                vm_is_trailing_zeros = _ryu_multiple_of_pow5(mm, q);
            } else {
                if _ryu_multiple_of_pow5(mp, q) { vp = vp - 1; }
            }
        }
    } else {
        let q = _ryu_log10_pow5(0 - e2);
        if (0 - e2) > 1 { q = q - 1; }
        e10 = q + e2;
        let i = 0 - e2 - q;
        let k = _ryu_pow5bits(i) - 125;
        let j = q - k;
        _ryu_compute_pow5(tbl, i, scratch);
        let mul_lo = scratch[0];
        let mul_hi = scratch[1];
        vr = _ryu_mul_shift_64(mv, mul_lo, mul_hi, j);
        vp = _ryu_mul_shift_64(mp, mul_lo, mul_hi, j);
        vm = _ryu_mul_shift_64(mm, mul_lo, mul_hi, j);
        if q <= 1 {
            vr_is_trailing_zeros = true;
            if accept_bounds {
                vm_is_trailing_zeros = mm_shift == 1;
            } else {
                vp = vp - 1;
            }
        } else if q < 63 {
            vr_is_trailing_zeros = _ryu_multiple_of_pow2(mv, q);
        }
    }
    let removed = 0;
    if vm_is_trailing_zeros || vr_is_trailing_zeros {
        while vp / 10 > vm / 10 {
            vm_is_trailing_zeros = vm_is_trailing_zeros && (vm % 10 == 0);
            vr_is_trailing_zeros = vr_is_trailing_zeros && (last_removed_digit == 0);
            last_removed_digit = vr % 10;
            vr = vr / 10; vp = vp / 10; vm = vm / 10;
            removed = removed + 1;
        }
        if vm_is_trailing_zeros {
            while vm % 10 == 0 {
                vr_is_trailing_zeros = vr_is_trailing_zeros && (last_removed_digit == 0);
                last_removed_digit = vr % 10;
                vr = vr / 10; vp = vp / 10; vm = vm / 10;
                removed = removed + 1;
            }
        }
        if vr_is_trailing_zeros && last_removed_digit == 5 && vr % 2 == 0 {
            last_removed_digit = 4;
        }
        if (vr == vm && (!accept_bounds || !vm_is_trailing_zeros)) || last_removed_digit >= 5 {
            vr = vr + 1;
        }
    } else {
        let round_up = false;
        let vpDiv100 = vp / 100;
        let vmDiv100 = vm / 100;
        if vpDiv100 > vmDiv100 {
            let vrDiv100 = vr / 100;
            round_up = (vr - 100 * vrDiv100) >= 50;
            vr = vrDiv100; vp = vpDiv100; vm = vmDiv100;
            removed = removed + 2;
        }
        while vp / 10 > vm / 10 {
            let vrDiv10 = vr / 10;
            round_up = (vr - 10 * vrDiv10) >= 5;
            vr = vrDiv10; vp = vp / 10; vm = vm / 10;
            removed = removed + 1;
        }
        if vr == vm || round_up { vr = vr + 1; }
    }
    out[0] = vr;
    out[1] = e10 + removed;
}


fun _ryu_formatted_length(mantissa: int, exponent: int, length: int, kk: int) -> int {
    if kk <= 0 { return 2 + (0 - kk) + length; }
    if kk < length { return length + 1; }
    return kk + 2;
}

fun _ryu_write_to(buf: ptr<int>, off: int, mantissa: int, exponent: int, length: int, kk: int, sign: int) -> int {
    let pos = off;
    if sign != 0 { buf[pos] = 45; pos = pos + 1; }
    if kk <= 0 {
        buf[pos] = 48; buf[pos + 1] = 46;
        pos = pos + 2;
        let z = 0;
        while z < 0 - kk { buf[pos] = 48; pos = pos + 1; z = z + 1; }
        let val = mantissa;
        let i = length - 1;
        while i >= 0 {
            buf[pos + i] = val % 10 + 48;
            val = val / 10; i = i - 1;
        }
        pos = pos + length;
    } else if kk < length {
        let val = mantissa;
        let i = length - 1;
        while i >= 0 {
            let wi = i;
            if i >= kk { wi = i + 1; }
            buf[pos + wi] = val % 10 + 48;
            val = val / 10; i = i - 1;
        }
        buf[pos + kk] = 46;
        pos = pos + length + 1;
    } else {
        let val = mantissa;
        let i = length - 1;
        while i >= 0 {
            buf[pos + i] = val % 10 + 48;
            val = val / 10; i = i - 1;
        }
        pos = pos + length;
        let z = 0;
        while z < kk - length { buf[pos] = 48; pos = pos + 1; z = z + 1; }
        buf[pos] = 46; buf[pos + 1] = 48;
        pos = pos + 2;
    }
    return pos;
}

fun _float_digit_count(f: float) -> int {
    let bits = __float_bits(f);
    let sign = _ushr(bits, 63);
    let ieee_exp = _ushr(bits, 52) & 2047;
    let ieee_mant = bits & 4503599627370495;
    if ieee_exp == 0 && ieee_mant == 0 { return 3 + sign; }
    if ieee_exp == 2047 {
        if ieee_mant != 0 { return 3; }
        return 3 + sign;
    }
    let scratch: array<int> = [0, 0];
    _ryu_d2d(ieee_mant, ieee_exp, scratch);
    let mantissa = scratch[0];
    let exponent = scratch[1];
    let length = _ryu_decimal_length17(mantissa);
    let kk = length + exponent;
    return sign + _ryu_formatted_length(mantissa, exponent, length, kk);
}

fun _float_write_to(buf: ptr<int>, off: int, f: float) -> int {
    let bits = __float_bits(f);
    let sign = _ushr(bits, 63);
    let ieee_exp = _ushr(bits, 52) & 2047;
    let ieee_mant = bits & 4503599627370495;
    if ieee_exp == 0 && ieee_mant == 0 {
        let pos = off;
        if sign != 0 { buf[pos] = 45; pos = pos + 1; }
        buf[pos] = 48; buf[pos + 1] = 46; buf[pos + 2] = 48;
        return pos + 3;
    }
    if ieee_exp == 2047 {
        if ieee_mant != 0 {
            buf[off] = 78; buf[off + 1] = 97; buf[off + 2] = 78;
            return off + 3;
        }
        let pos = off;
        if sign != 0 { buf[pos] = 45; pos = pos + 1; }
        buf[pos] = 105; buf[pos + 1] = 110; buf[pos + 2] = 102;
        return pos + 3;
    }
    let scratch: array<int> = [0, 0];
    _ryu_d2d(ieee_mant, ieee_exp, scratch);
    let mantissa = scratch[0];
    let exponent = scratch[1];
    let length = _ryu_decimal_length17(mantissa);
    let kk = length + exponent;
    return _ryu_write_to(buf, off, mantissa, exponent, length, kk, sign);
}


// Value to String Conversion
// ============================================================================

// Internal: convert integer to string (single heap allocation).
fun _int_to_string(n: int) -> string {
    if n == 0 {
        return "0";
    }
    let dcount = _int_digit_count(n);
    let data = __alloc_heap(dcount);
    _int_write_to(data, 0, n);
    return __alloc_string(data, dcount);
}

// Internal: convert float to string (single heap allocation).
fun _float_to_string(f: float) -> string {
    let dcount = _float_digit_count(f);
    let data = __alloc_heap(dcount);
    _float_write_to(data, 0, f);
    return __alloc_string(data, dcount);
}

// Internal: convert bool to string.
fun _bool_to_string(b: bool) -> string {
    if b { return "true"; }
    return "false";
}

// ============================================================================
// ToString Interface
// ============================================================================

interface ToString {
    fun to_string(self) -> string;
}

impl ToString for int {
    fun to_string(self) -> string {
        return _int_to_string(self);
    }
}

impl ToString for float {
    fun to_string(self) -> string {
        return _float_to_string(self);
    }
}

impl ToString for bool {
    fun to_string(self) -> string {
        return _bool_to_string(self);
    }
}

impl ToString for string {
    fun to_string(self) -> string {
        return self;
    }
}

// Zero-pad an integer to 2 digits.
fun _pad2(n: int) -> string {
    if n < 10 {
        return "0" + n.to_string();
    }
    return n.to_string();
}

// Zero-pad an integer to 4 digits.
fun _pad4(n: int) -> string {
    if n < 10 {
        return "000" + n.to_string();
    }
    if n < 100 {
        return "00" + n.to_string();
    }
    if n < 1000 {
        return "0" + n.to_string();
    }
    return n.to_string();
}

// Check if a year is a leap year.
fun _is_leap_year(y: int) -> bool {
    return y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
}

// Get number of days in a month (1-indexed).
fun _days_in_month(y: int, m: int) -> int {
    if m == 2 {
        if _is_leap_year(y) {
            return 29;
        }
        return 28;
    }
    if m == 4 || m == 6 || m == 9 || m == 11 {
        return 30;
    }
    return 31;
}

// Format epoch seconds as "YYYY-MM-DD HH:MM:SS" (UTC).
// Uses civil_from_days algorithm for date calculation.
fun time_format(epoch_secs: int) -> string {
    // Euclidean division for correct negative handling
    let days = epoch_secs / 86400;
    let day_secs = epoch_secs - days * 86400;
    if day_secs < 0 {
        days = days - 1;
        day_secs = day_secs + 86400;
    }

    let hour = day_secs / 3600;
    let minute = (day_secs - hour * 3600) / 60;
    let second = day_secs - hour * 3600 - minute * 60;

    // civil_from_days: convert days since 1970-01-01 to y/m/d
    let z = days + 719468;
    let era = z / 146097;
    if z < 0 {
        era = (z - 146096) / 146097;
    }
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y_base = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + 3;
    if mp >= 10 {
        m = mp - 9;
    }
    let y = y_base;
    if m <= 2 {
        y = y_base + 1;
    }

    return _pad4(y) + "-" + _pad2(m) + "-" + _pad2(d) + " " + _pad2(hour) + ":" + _pad2(minute) + ":" + _pad2(second);
}

// ============================================================================
// String Operations
// ============================================================================

// Compare two strings by content (length + data array elements).
@inline
fun _string_eq(a: string, b: string) -> bool {
    let a_len = __heap_load(a, 1);
    let b_len = __heap_load(b, 1);
    if a_len != b_len {
        return false;
    }
    let a_ptr = __heap_load(a, 0);
    let b_ptr = __heap_load(b, 0);
    let i = 0;
    while i < a_len {
        if __heap_load(a_ptr, i) != __heap_load(b_ptr, i) {
            return false;
        }
        i = i + 1;
    }
    return true;
}

// Concatenate two strings by copying character data into a new string.
@inline
fun string_concat(a: string, b: string) -> string {
    let a_ptr = a.data;
    let a_len = a.len;
    let b_ptr = b.data;
    let b_len = b.len;
    let total = a_len + b_len;
    let data = __alloc_heap(total);
    let i = 0;
    while i < a_len {
        data[i] = a_ptr[i];
        i = i + 1;
    }
    while i < total {
        data[i] = b_ptr[i - a_len];
        i = i + 1;
    }
    return __alloc_string(data, total);
}

// Join all strings in an array into a single string.
// Pre-allocates the result buffer based on total length, then copies all parts.
@inline
fun string_join(parts: array<string>) -> string {
    let n = len(parts);
    let total = 0;
    let i = 0;
    while i < n {
        total = total + len(parts[i]);
        i = i + 1;
    }
    let data = __alloc_heap(total);
    let off = 0;
    i = 0;
    while i < n {
        let s = parts[i];
        let s_ptr = s.data;
        let s_len = s.len;
        let j = 0;
        while j < s_len {
            data[off] = s_ptr[j];
            off = off + 1;
            j = j + 1;
        }
        i = i + 1;
    }
    return __alloc_string(data, total);
}

// ============================================================================
// High-level I/O Functions
// ============================================================================

// Print a string to stdout without a newline.
fun print_str(s: string) {
    let n = len(s);
    write(1, s, n);
}

// Print a value to stdout with a trailing newline.
// Requires the value's type to implement the ToString interface.
fun print<T: ToString>(v: T) {
    print_str(v.to_string());
    print_str("\n");
}

// Recursively format a dyn-like wrapper [type_info_ref, raw_value].
// Uses type_info heap layout to recurse into containers and structs.
// type_info layout: [tag_id, type_name, fc, ...field_names, ...field_td_refs, aux_count, ...aux_td_refs]
fun _any_to_string(d: any) -> string {
    let ti = __heap_load(d, 0);
    let tn: string = __heap_load(ti, 1);
    let fc: int = __heap_load(ti, 2);

    // Primitives (no fields)
    if fc == 0 {
        let raw = __heap_load(d, 1);
        if tn == "int" {
            return _int_to_string(raw);
        }
        if tn == "float" {
            return _float_to_string(raw);
        }
        if tn == "bool" {
            return _bool_to_string(raw);
        }
        if tn == "string" {
            return raw;
        }
        if tn == "nil" {
            return "nil";
        }
        return __value_to_string(raw);
    }

    let raw = __heap_load(d, 1);
    let ac: int = __heap_load(ti, 3 + 2 * fc);
    let fn0: string = __heap_load(ti, 3);

    // Vec (fc == 3, data/len/cap)
    if fc == 3 {
        let fn1: string = __heap_load(ti, 4);
        let fn2: string = __heap_load(ti, 5);
        if fn0 == "data" && fn1 == "len" && fn2 == "cap" && ac >= 1 {
            let data = __heap_load(raw, 0);
            let vec_len: int = __heap_load(raw, 1);
            if vec_len == 0 {
                return "[]";
            }
            let elem_td = __heap_load(ti, 3 + 2 * fc + 1);
            let result = "[";
            let i = 0;
            while i < vec_len {
                if i > 0 {
                    result = result + ", ";
                }
                let wrapper = __alloc_heap(2);
                __heap_store(wrapper, 0, elem_td);
                __heap_store(wrapper, 1, __heap_load(data, i));
                result = result + _any_to_string(wrapper);
                i = i + 1;
            }
            return result + "]";
        }
        // Map (fc == 3, hm_buckets/hm_size/hm_capacity)
        if fn0 == "hm_buckets" && ac >= 2 {
            let buckets = __heap_load(raw, 0);
            let map_size: int = __heap_load(raw, 1);
            let map_cap: int = __heap_load(raw, 2);
            if map_size == 0 {
                return "{}";
            }
            let key_td = __heap_load(ti, 3 + 2 * fc + 1);
            let val_td = __heap_load(ti, 3 + 2 * fc + 2);
            let result = "{";
            let first = true;
            let bi = 0;
            while bi < map_cap {
                let entry_ptr = __heap_load(buckets, bi);
                while entry_ptr != 0 {
                    if !first {
                        result = result + ", ";
                    }
                    first = false;
                    let kw = __alloc_heap(2);
                    __heap_store(kw, 0, key_td);
                    __heap_store(kw, 1, __heap_load(entry_ptr, 0));
                    let vw = __alloc_heap(2);
                    __heap_store(vw, 0, val_td);
                    __heap_store(vw, 1, __heap_load(entry_ptr, 1));
                    result = result + _any_to_string(kw) + ": " + _any_to_string(vw);
                    entry_ptr = __heap_load(entry_ptr, 2);
                }
                bi = bi + 1;
            }
            return result + "}";
        }
    }

    // Array (fc == 2, data/len)
    if fc == 2 && fn0 == "data" && ac >= 1 {
        let fn1: string = __heap_load(ti, 4);
        if fn1 == "len" {
            let data = __heap_load(raw, 0);
            let arr_len: int = __heap_load(raw, 1);
            if arr_len == 0 {
                return "[]";
            }
            let elem_td = __heap_load(ti, 3 + 2 * fc + 1);
            let result = "[";
            let i = 0;
            while i < arr_len {
                if i > 0 {
                    result = result + ", ";
                }
                let wrapper = __alloc_heap(2);
                __heap_store(wrapper, 0, elem_td);
                __heap_store(wrapper, 1, __heap_load(data, i));
                result = result + _any_to_string(wrapper);
                i = i + 1;
            }
            return result + "]";
        }
    }

    // Struct with named fields
    let result = tn + " { ";
    let i = 0;
    while i < fc {
        if i > 0 {
            result = result + ", ";
        }
        let field_name: string = __heap_load(ti, 3 + i);
        let field_td = __heap_load(ti, 3 + fc + i);
        let field_val = __heap_load(raw, i);
        let wrapper = __alloc_heap(2);
        __heap_store(wrapper, 0, field_td);
        __heap_store(wrapper, 1, field_val);
        result = result + field_name + ": " + _any_to_string(wrapper);
        i = i + 1;
    }
    return result + " }";
}

// Convert a dyn value to its string representation using match dyn.
// Dispatches to type-specific moca functions for primitives.
// Falls back to _any_to_string for containers and structs.
fun _value_to_string(v: dyn) -> string {
    match dyn v {
        x: int => { return _int_to_string(x); }
        x: float => { return _float_to_string(x); }
        x: bool => { return _bool_to_string(x); }
        x: string => { return x; }
        _ => {
            return _any_to_string(v);
        }
    }
}

fun debug(v: dyn) -> string {
    return _value_to_string(v);
}

// Print a string to stderr without a newline.
fun eprint_str(s: string) {
    let n = len(s);
    write(2, s, n);
}

// ============================================================================
// Testing / Assertion Functions
// ============================================================================

// Assert that a condition is true. If false, throws an error with the given message.
fun assert(condition: bool, msg: string) {
    if !condition {
        throw msg;
    }
}

// Assert that two values are equal. If not equal, throws an error with the given message.
// Uses to_string for comparison, so works with any type that can be converted to string.
fun assert_eq(actual: int, expected: int, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + expected.to_string() + ", actual: " + actual.to_string() + ")";
    }
}

// Assert that two strings are equal.
fun assert_eq_str(actual: string, expected: string, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + expected + ", actual: " + actual + ")";
    }
}

// Assert that two booleans are equal.
fun assert_eq_bool(actual: bool, expected: bool, msg: string) {
    if actual != expected {
        throw msg + " (expected: " + expected.to_string() + ", actual: " + actual.to_string() + ")";
    }
}

// ============================================================================
// Math Functions
// ============================================================================

fun abs(x: int) -> int {
    if x < 0 {
        return -x;
    }
    return x;
}

fun max(a: int, b: int) -> int {
    if a > b {
        return a;
    }
    return b;
}

fun min(a: int, b: int) -> int {
    if a < b {
        return a;
    }
    return b;
}

// Internal: Convert float to int (truncation toward zero)
fun _float_to_int(x: float) -> int {
    return asm(x) -> i64 {
        __emit("I64TruncF64S");
    };
}

// Absolute value of a float
fun abs_f(x: float) -> float {
    if x < 0.0 {
        return 0.0 - x;
    }
    return x;
}

// Square root using Newton's method (Babylonian method)
fun sqrt_f(x: float) -> float {
    if x <= 0.0 {
        return 0.0;
    }
    let guess = x;
    // Better initial guess: halve repeatedly until reasonable
    if x > 1.0 {
        guess = x / 2.0;
    }
    let i = 0;
    while i < 20 {
        guess = (guess + x / guess) / 2.0;
        i = i + 1;
    }
    return guess;
}

// Floor: largest integer <= x, returned as float
fun floor_f(x: float) -> float {
    let t = _float_to_int(x);
    let tf = _int_to_float(t);
    // _float_to_int truncates toward zero, so for negative non-integers we need -1
    if tf > x {
        return tf - 1.0;
    }
    return tf;
}

// Float modulo (equivalent to fmod)
fun fmod_f(x: float, y: float) -> float {
    return x - floor_f(x / y) * y;
}

// Sine function using Taylor series with range reduction
fun sin_f(x: float) -> float {
    let pi = 3.14159265358979323846;
    let two_pi = 6.28318530717958647692;

    // Range reduction to [-pi, pi]
    let a = fmod_f(x, two_pi);
    if a > pi {
        a = a - two_pi;
    }
    if a < 0.0 - pi {
        a = a + two_pi;
    }

    // Taylor series: sin(a) = a - a^3/3! + a^5/5! - a^7/7! + ...
    let term = a;
    let sum = a;
    let i = 1;
    while i < 12 {
        let n = _int_to_float(2 * i) * (_int_to_float(2 * i) + 1.0);
        term = 0.0 - term * a * a / n;
        sum = sum + term;
        i = i + 1;
    }
    return sum;
}

// Cosine function: cos(x) = sin(x + pi/2)
fun cos_f(x: float) -> float {
    return sin_f(x + 1.5707963267948966);
}

// ============================================================================
// String Functions
// ============================================================================

fun str_len(s: string) -> int {
    return len(s);
}

fun str_contains(haystack: string, needle: string) -> bool {
    let haystack_len = len(haystack);
    let needle_len = len(needle);

    if needle_len == 0 {
        return true;
    }
    if needle_len > haystack_len {
        return false;
    }

    let i = 0;
    while i <= haystack_len - needle_len {
        let j = 0;
        let found = true;
        while j < needle_len {
            if haystack[i + j] != needle[j] {
                found = false;
                j = needle_len;
            } else {
                j = j + 1;
            }
        }
        if found {
            return true;
        }
        i = i + 1;
    }
    return false;
}

// Find the index of needle in haystack, returns -1 if not found
fun str_index_of(haystack: string, needle: string) -> int {
    let haystack_len = len(haystack);
    let needle_len = len(needle);

    if needle_len == 0 {
        return 0;
    }
    if needle_len > haystack_len {
        return -1;
    }

    let i = 0;
    while i <= haystack_len - needle_len {
        let j = 0;
        let found = true;
        while j < needle_len {
            if haystack[i + j] != needle[j] {
                found = false;
                j = needle_len;
            } else {
                j = j + 1;
            }
        }
        if found {
            return i;
        }
        i = i + 1;
    }
    return -1;
}

// ============================================================================
// Array Functions (fixed-length array using heap intrinsics)
// ============================================================================

// Array<T> - Fixed-length array implementation.
// Layout: [data, len]
struct Array<T> {
    data: ptr<T>,
    len: int
}

impl<T> Array<T> {
    // Get a value at the specified index
    fun get(self, index: int) -> T {
        return self.data[index];
    }

    // Set a value at the specified index
    fun set(self, index: int, value: T) {
        self.data[index] = value;
    }

    // Get the length of the array
    fun len(self) -> int {
        return self.len;
    }
}

// ============================================================================
// Vector Functions (low-level implementation using heap intrinsics)
// ============================================================================

// Vec<T> - Generic vector (dynamic array) implementation.
// Layout: [data, len, cap]
struct Vec<T> {
    data: ptr<T>,
    len: int,
    cap: int
}

impl<T> Vec<T> {
    // Create a new empty vector.
    fun `new`() -> Vec<T> {
        return Vec<T> { data: __null_ptr(), len: 0, cap: 0 };
    }

    // Create a vector with pre-set capacity.
    fun with_capacity(cap: int) -> Vec<T> {
        return Vec<T> { data: __null_ptr(), len: 0, cap: cap };
    }

    // Create an uninitialized vector with specified length (for desugar).
    // The vector is allocated with the given capacity and length is set to capacity.
    // Elements are uninitialized and must be set before use.
    fun uninit(cap: int) -> Vec<T> {
        if cap == 0 {
            return Vec<T> { data: __null_ptr(), len: 0, cap: 0 };
        }
        let d = __alloc_heap(cap);
        return Vec<T> { data: d, len: cap, cap: cap };
    }

    // Push a value to the end of the vector
    fun push(self, value: T) {
        if self.len >= self.cap {
            // Need to grow
            let new_cap = self.cap * 2;
            if new_cap < 8 {
                new_cap = 8;
            }
            let new_data = __alloc_heap(new_cap);

            // Copy old data if data is not null
            if self.data != __null_ptr() {
                let i = 0;
                while i < self.len {
                    let val = self.data[i];
                    new_data[i] = val;
                    i = i + 1;
                }
            }

            // Update vector header
            self.data = new_data;
            self.cap = new_cap;
        }

        // Store the value at data[len]
        self.data[self.len] = value;
        // Increment len
        self.len = self.len + 1;
    }

    // Pop a value from the end of the vector
    // Returns the popped value, throws if vector is empty.
    fun pop(self) -> T {
        if self.len == 0 {
            throw "cannot pop from empty vector";
        }

        self.len = self.len - 1;
        let value = self.data[self.len];

        return value;
    }

    // Get a value at the specified index
    @inline
    fun get(self, index: int) -> T {
        return self.data[index];
    }

    // Set a value at the specified index
    @inline
    fun set(self, index: int, value: T) {
        self.data[index] = value;
    }

    // Get the length of the vector
    fun len(self) -> int {
        return self.len;
    }

    // Get the first element of the vector
    fun first(self) -> T {
        return self.data[0];
    }
}

// Associated functions for vec<T> (syntax sugar for Vec<T>)
// ============================================================================
// Map Functions (HashMap implementation using chaining)
// ============================================================================

// HashMapEntry struct - represents a key-value pair in the map.
// Layout: [hm_key, hm_value, hm_next]
// hm_next: pointer to next entry in the chain (0 if end of chain)
struct HashMapEntry<K, V> {
    hm_key: K,
    hm_value: V,
    hm_next: ptr<any>
}

// Map<K, V> - Generic hash map implementation.
// Layout: [hm_buckets, hm_size, hm_capacity]
// hm_buckets: pointer to array of bucket heads
// hm_size: number of entries in the map
// hm_capacity: number of buckets
struct Map<K, V> {
    hm_buckets: ptr<any>,
    hm_size: int,
    hm_capacity: int
}

// Hash function for integers - uses the value directly
fun _map_hash_int(key: int) -> int {
    if key < 0 {
        return -key;
    }
    return key;
}

// Hash function for strings - DJB2 algorithm
fun _map_hash_string(key: string) -> int {
    let hash = 5381;
    let n = len(key);
    let i = 0;
    while i < n {
        let c = key[i];
        // hash = hash * 33 + c
        hash = hash * 33 + c;
        i = i + 1;
    }
    if hash < 0 {
        return -hash;
    }
    return hash;
}

impl<K, V> Map<K, V> {
    // Internal: Find entry by key in a bucket chain (int key)
    fun _find_entry_int(self, key: int) -> int {
        let bucket_idx = _map_hash_int(key) % self.hm_capacity;
        let entry_ptr = self.hm_buckets[bucket_idx];

        while entry_ptr != 0 {
            let entry_key = __heap_load(entry_ptr, 0);
            if entry_key == key {
                return entry_ptr;
            }
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        return 0;
    }

    // Internal: Find entry by key in a bucket chain (string key)
    fun _find_entry_string(self, key: string) -> int {
        let bucket_idx = _map_hash_string(key) % self.hm_capacity;
        let entry_ptr = self.hm_buckets[bucket_idx];

        while entry_ptr != 0 {
            let entry_key = __heap_load(entry_ptr, 0);
            if entry_key == key {
                return entry_ptr;
            }
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        return 0;
    }

    // Internal: Rehash the map when load factor exceeds 0.75 (int keys)
    fun _rehash_int(self) {
        let old_capacity = self.hm_capacity;
        let old_buckets = self.hm_buckets;
        let new_capacity = old_capacity * 2;
        let new_buckets = __alloc_heap(new_capacity);

        let i = 0;
        while i < new_capacity {
            new_buckets[i] = 0;
            i = i + 1;
        }

        i = 0;
        while i < old_capacity {
            let entry_ptr = old_buckets[i];
            while entry_ptr != 0 {
                let key = __heap_load(entry_ptr, 0);
                let next_ptr = __heap_load(entry_ptr, 2);

                let new_bucket_idx = _map_hash_int(key) % new_capacity;

                let old_head = new_buckets[new_bucket_idx];
                __heap_store(entry_ptr, 2, old_head);
                new_buckets[new_bucket_idx] = entry_ptr;

                entry_ptr = next_ptr;
            }
            i = i + 1;
        }

        self.hm_buckets = new_buckets;
        self.hm_capacity = new_capacity;
    }

    // Internal: Rehash for string keys
    fun _rehash_string(self) {
        let old_capacity = self.hm_capacity;
        let old_buckets = self.hm_buckets;
        let new_capacity = old_capacity * 2;
        let new_buckets = __alloc_heap(new_capacity);

        let i = 0;
        while i < new_capacity {
            new_buckets[i] = 0;
            i = i + 1;
        }

        i = 0;
        while i < old_capacity {
            let entry_ptr = old_buckets[i];
            while entry_ptr != 0 {
                let key = __heap_load(entry_ptr, 0);
                let next_ptr = __heap_load(entry_ptr, 2);

                let new_bucket_idx = _map_hash_string(key) % new_capacity;

                let old_head = new_buckets[new_bucket_idx];
                __heap_store(entry_ptr, 2, old_head);
                new_buckets[new_bucket_idx] = entry_ptr;

                entry_ptr = next_ptr;
            }
            i = i + 1;
        }

        self.hm_buckets = new_buckets;
        self.hm_capacity = new_capacity;
    }

    // Create a new empty map
    fun `new`() -> Map<K, V> {
        let capacity = 16;
        let buckets = __alloc_heap(capacity);
        // Initialize all buckets to 0 (empty)
        let i = 0;
        while i < capacity {
            buckets[i] = 0;
            i = i + 1;
        }
        return Map<K, V> { hm_buckets: buckets, hm_size: 0, hm_capacity: capacity };
    }

    // Create an uninitialized empty map (for desugar).
    // Same as `new()` - elements will be added via put.
    fun uninit() -> Map<K, V> {
        let capacity = 16;
        let buckets = __alloc_heap(capacity);
        let i = 0;
        while i < capacity {
            buckets[i] = 0;
            i = i + 1;
        }
        return Map<K, V> { hm_buckets: buckets, hm_size: 0, hm_capacity: capacity };
    }

    // Put a key-value pair into the map (int key version)
    fun put_int(self, key: int, val: V) {
        // Check if key already exists
        let existing = self._find_entry_int(key);
        if existing != 0 {
            // Update existing entry
            __heap_store(existing, 1, val);
            return;
        }

        // Check if we need to rehash (load factor > 0.75)
        let load = self.hm_size * 4;
        let threshold = self.hm_capacity * 3;
        if load >= threshold {
            self._rehash_int();
        }

        // Insert at head of bucket
        let bucket_idx = _map_hash_int(key) % self.hm_capacity;
        let old_head = self.hm_buckets[bucket_idx];
        let entry = HashMapEntry<int, V> { hm_key: key, hm_value: val, hm_next: old_head };
        self.hm_buckets[bucket_idx] = entry;

        self.hm_size = self.hm_size + 1;
    }

    // Put a key-value pair into the map (string key version)
    fun put_string(self, key: string, val: V) {
        // Check if key already exists
        let existing = self._find_entry_string(key);
        if existing != 0 {
            // Update existing entry
            __heap_store(existing, 1, val);
            return;
        }

        // Check if we need to rehash (load factor > 0.75)
        let load = self.hm_size * 4;
        let threshold = self.hm_capacity * 3;
        if load >= threshold {
            self._rehash_string();
        }

        // Insert at head of bucket
        let bucket_idx = _map_hash_string(key) % self.hm_capacity;
        let old_head = self.hm_buckets[bucket_idx];
        let entry = HashMapEntry<string, V> { hm_key: key, hm_value: val, hm_next: old_head };
        self.hm_buckets[bucket_idx] = entry;

        self.hm_size = self.hm_size + 1;
    }

    // Get a value from the map by int key
    // Throws if key not found
    fun get_int(self, key: int) -> V {
        let entry_ptr = self._find_entry_int(key);
        if entry_ptr == 0 {
            throw "key not found";
        }
        return __heap_load(entry_ptr, 1);
    }

    // Get a value from the map by string key
    // Throws if key not found
    fun get_string(self, key: string) -> V {
        let entry_ptr = self._find_entry_string(key);
        if entry_ptr == 0 {
            throw "key not found";
        }
        return __heap_load(entry_ptr, 1);
    }

    // Check if the map contains a key (int version)
    fun contains_int(self, key: int) -> bool {
        return self._find_entry_int(key) != 0;
    }

    // Check if the map contains a key (string version)
    fun contains_string(self, key: string) -> bool {
        return self._find_entry_string(key) != 0;
    }

    // Remove an entry from the map by int key
    // Returns true if the key was found and removed, false otherwise
    fun remove_int(self, key: int) -> bool {
        let bucket_idx = _map_hash_int(key) % self.hm_capacity;
        let entry_ptr = self.hm_buckets[bucket_idx];
        let prev_ptr = 0;

        while entry_ptr != 0 {
            let entry_key = __heap_load(entry_ptr, 0);
            if entry_key == key {
                // Found the entry, remove it
                let next_ptr = __heap_load(entry_ptr, 2);
                if prev_ptr == 0 {
                    // Entry is head of bucket
                    self.hm_buckets[bucket_idx] = next_ptr;
                } else {
                    // Entry is in middle/end of chain
                    __heap_store(prev_ptr, 2, next_ptr);
                }
                self.hm_size = self.hm_size - 1;
                return true;
            }
            prev_ptr = entry_ptr;
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        return false;
    }

    // Remove an entry from the map by string key
    // Returns true if the key was found and removed, false otherwise
    fun remove_string(self, key: string) -> bool {
        let bucket_idx = _map_hash_string(key) % self.hm_capacity;
        let entry_ptr = self.hm_buckets[bucket_idx];
        let prev_ptr = 0;

        while entry_ptr != 0 {
            let entry_key = __heap_load(entry_ptr, 0);
            if entry_key == key {
                // Found the entry, remove it
                let next_ptr = __heap_load(entry_ptr, 2);
                if prev_ptr == 0 {
                    // Entry is head of bucket
                    self.hm_buckets[bucket_idx] = next_ptr;
                } else {
                    // Entry is in middle/end of chain
                    __heap_store(prev_ptr, 2, next_ptr);
                }
                self.hm_size = self.hm_size - 1;
                return true;
            }
            prev_ptr = entry_ptr;
            entry_ptr = __heap_load(entry_ptr, 2);
        }
        return false;
    }

    // Get all keys from the map as a vector
    fun keys(self) -> Vec<K> {
        let result: Vec<K> = Vec<K> { data: __null_ptr(), len: 0, cap: 0 };
        let i = 0;
        while i < self.hm_capacity {
            let entry_ptr = self.hm_buckets[i];
            while entry_ptr != 0 {
                let key = __heap_load(entry_ptr, 0);
                result.push(key);
                entry_ptr = __heap_load(entry_ptr, 2);
            }
            i = i + 1;
        }
        return result;
    }

    // Get all values from the map as a vector
    fun values(self) -> Vec<V> {
        let result: Vec<V> = Vec<V> { data: __null_ptr(), len: 0, cap: 0 };
        let i = 0;
        while i < self.hm_capacity {
            let entry_ptr = self.hm_buckets[i];
            while entry_ptr != 0 {
                let val = __heap_load(entry_ptr, 1);
                result.push(val);
                entry_ptr = __heap_load(entry_ptr, 2);
            }
            i = i + 1;
        }
        return result;
    }

    // Get the size of the map
    fun len(self) -> int {
        return self.hm_size;
    }
}

// ============================================================================
// Random Number Generation (LCG - Linear Congruential Generator)
// ============================================================================

// Internal: Convert int to float using inline assembly
fun _int_to_float(n: int) -> float {
    return asm(n) -> f64 {
        __emit("F64ConvertI64S");
    };
}

// Rand - Pseudo-random number generator using LCG algorithm.
// LCG parameters: a = 1103515245, c = 12345, m = 2147483648 (2^31)
//
// Usage:
//   let rng = Rand::new(42);
//   print(rng.int(1, 100));
//   print(rng.float());
struct Rand {
    _seed: int
}

impl Rand {
    // Create a new random number generator with the given seed.
    fun `new`(seed: int) -> Rand {
        return Rand { _seed: seed };
    }

    // Set the seed for the random number generator.
    fun set_seed(self, n: int) {
        self._seed = n;
    }

    // Generate the next raw random integer in [0, 2^31).
    fun next(self) -> int {
        self._seed = (self._seed * 1103515245 + 12345) % 2147483648;
        if self._seed < 0 {
            self._seed = -self._seed;
        }
        return self._seed;
    }

    // Generate a random integer in [min_val, max_val].
    // Throws an error if min_val > max_val.
    // Uses scaling (upper bits) instead of modulo to avoid LCG lower-bit bias.
    fun int(self, min_val: int, max_val: int) -> int {
        if min_val > max_val {
            throw "rand_int: min must be <= max";
        }
        let r = self.next();
        let range = max_val - min_val + 1;
        return min_val + r * range / 2147483648;
    }

    // Generate a random float in [0.0, 1.0).
    fun float(self) -> float {
        let r = self.next();
        return _int_to_float(r) / 2147483648.0;
    }
}

// ============================================================================
// Parsing Functions
// ============================================================================

// Check if a byte is a whitespace character (space, tab, newline, carriage return)
fun _is_whitespace(c: int) -> bool {
    return c == 32 || c == 9 || c == 10 || c == 13;
}

// Check if a byte is a digit ('0'-'9')
fun _is_digit(c: int) -> bool {
    return c >= 48 && c <= 57;
}

// Parse a string to an integer.
// Handles leading/trailing whitespace and optional negative sign.
// Throws an error if the string cannot be parsed as an integer.
fun std_parse_int(s: string) -> int {
    let n = len(s);
    let i = 0;

    // Skip leading whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    if i >= n {
        throw "cannot parse empty string as int";
    }

    // Check for negative sign
    let negative = false;
    if s[i] == 45 {
        negative = true;
        i = i + 1;
    }

    if i >= n || !_is_digit(s[i]) {
        throw "cannot parse '" + s + "' as int";
    }

    // Parse digits
    let result = 0;
    while i < n && _is_digit(s[i]) {
        let digit = s[i] - 48;
        result = result * 10 + digit;
        i = i + 1;
    }

    // Skip trailing whitespace
    while i < n && _is_whitespace(s[i]) {
        i = i + 1;
    }

    // Check for trailing non-whitespace characters
    if i < n {
        throw "cannot parse '" + s + "' as int";
    }

    if negative {
        return -result;
    }
    return result;
}

// ============================================================================
// Sort Functions (Quicksort with median-of-three pivot)
// ============================================================================

// Internal: swap two elements in a vec<int>
fun _sort_int_swap(v: Vec<int>, i: int, j: int) {
    let tmp = v[i];
    v[i] = v[j];
    v[j] = tmp;
}

// Internal: quicksort implementation for vec<int>
fun _sort_int_impl(v: Vec<int>, low: int, high: int) {
    if low >= high {
        return;
    }

    // Median-of-three pivot selection (only for 3+ elements)
    if high - low >= 2 {
        let mid = low + (high - low) / 2;
        if v[low] > v[mid] {
            _sort_int_swap(v, low, mid);
        }
        if v[low] > v[high] {
            _sort_int_swap(v, low, high);
        }
        if v[mid] > v[high] {
            _sort_int_swap(v, mid, high);
        }
        // v[mid] is the median, swap to high for Lomuto partition
        _sort_int_swap(v, mid, high);
    }

    // Lomuto partition with pivot at v[high]
    let pivot = v[high];
    let i = low;
    let j = low;
    while j < high {
        if v[j] <= pivot {
            _sort_int_swap(v, i, j);
            i = i + 1;
        }
        j = j + 1;
    }
    _sort_int_swap(v, i, high);

    // Recurse on both sides
    if i > low {
        _sort_int_impl(v, low, i - 1);
    }
    _sort_int_impl(v, i + 1, high);
}

// Sort a vec<int> in-place in ascending order using quicksort.
fun sort_int(v: Vec<int>) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    _sort_int_impl(v, 0, n - 1);
}

// Internal: swap two elements in a vec<float>
fun _sort_float_swap(v: Vec<float>, i: int, j: int) {
    let tmp = v[i];
    v[i] = v[j];
    v[j] = tmp;
}

// Internal: quicksort implementation for vec<float>
fun _sort_float_impl(v: Vec<float>, low: int, high: int) {
    if low >= high {
        return;
    }

    // Median-of-three pivot selection (only for 3+ elements)
    if high - low >= 2 {
        let mid = low + (high - low) / 2;
        if v[low] > v[mid] {
            _sort_float_swap(v, low, mid);
        }
        if v[low] > v[high] {
            _sort_float_swap(v, low, high);
        }
        if v[mid] > v[high] {
            _sort_float_swap(v, mid, high);
        }
        _sort_float_swap(v, mid, high);
    }

    // Lomuto partition with pivot at v[high]
    let pivot = v[high];
    let i = low;
    let j = low;
    while j < high {
        if v[j] <= pivot {
            _sort_float_swap(v, i, j);
            i = i + 1;
        }
        j = j + 1;
    }
    _sort_float_swap(v, i, high);

    // Recurse on both sides
    if i > low {
        _sort_float_impl(v, low, i - 1);
    }
    _sort_float_impl(v, i + 1, high);
}

// Sort a vec<float> in-place in ascending order using quicksort.
fun sort_float(v: Vec<float>) {
    let n = v.len();
    if n <= 1 {
        return;
    }
    _sort_float_impl(v, 0, n - 1);
}

// ============================================================================
// Dynamic Type (dyn) Operations
// ============================================================================
// dyn values are 2-slot heap objects: [type_info_ref, value]
//
// type_info is a heap object:
//   slot 0: tag_id (int, string pool index for fast matching)
//   slot 1: type_name (string, e.g. "int", "Point")
//   slot 2: field_count (int, n; 0 for primitives)
//   slot 3..3+n: field_names (strings)
//   slot 3+n..3+2n: field_type_descriptor_refs (refs to type_info objects)

// Get the type name of a dyn value.
fun __dyn_type_name(d: dyn) -> string {
    let type_info = __heap_load(d, 0);
    return __heap_load(type_info, 1);
}

// Get the number of fields of a dyn value (0 for primitives).
fun __dyn_field_count(d: dyn) -> int {
    let type_info = __heap_load(d, 0);
    return __heap_load(type_info, 2);
}

// Get the name of the i-th field of a dyn value.
fun __dyn_field_name(d: dyn, i: int) -> string {
    let type_info = __heap_load(d, 0);
    return __heap_load(type_info, 3 + i);
}

// Get the i-th field value of a dyn value as a new dyn.
// Uses the field type descriptor stored in the type_info to wrap the field value.
fun __dyn_field_value(d: dyn, i: int) -> any {
    let type_info = __heap_load(d, 0);
    let field_count: int = __heap_load(type_info, 2);
    let field_td_ref = __heap_load(type_info, 3 + field_count + i);
    let struct_val = __heap_load(d, 1);
    let field_val = __heap_load(struct_val, i);
    let result = __alloc_heap(2);
    __heap_store(result, 0, field_td_ref);
    __heap_store(result, 1, field_val);
    return result;
}

// ============================================================================
// Dyn-based Generic Formatter
// ============================================================================

// Convert a dyn value to its string representation.
// Handles primitives and structs recursively.
// Output format: "StructName { field1: value1, field2: value2 }"
fun _dyn_to_string(d: any) -> string {
    let type_name: string = __dyn_type_name(d);
    let field_count: int = __dyn_field_count(d);

    // Primitive types (field_count == 0)
    if field_count == 0 {
        if type_name == "int" {
            let v: int = __heap_load(d, 1);
            return _int_to_string(v);
        }
        if type_name == "float" {
            let v: float = __heap_load(d, 1);
            return _float_to_string(v);
        }
        if type_name == "bool" {
            let v: bool = __heap_load(d, 1);
            return _bool_to_string(v);
        }
        if type_name == "string" {
            let v: string = __heap_load(d, 1);
            return "\"" + v + "\"";
        }
        if type_name == "nil" {
            return "nil";
        }
        return type_name;
    }

    // Struct types: "TypeName { field1: value1, field2: value2 }"
    let result = type_name + " { ";
    let i = 0;
    while i < field_count {
        if i > 0 {
            result = result + ", ";
        }
        let fname: string = __dyn_field_name(d, i);
        let fval = __dyn_field_value(d, i);
        result = result + fname + ": " + _dyn_to_string(fval);
        i = i + 1;
    }
    result = result + " }";
    return result;
}

// Print a dyn value's string representation to stdout with a trailing newline.
fun inspect(d: dyn) {
    print_str(_dyn_to_string(d));
    print_str("\n");
}
