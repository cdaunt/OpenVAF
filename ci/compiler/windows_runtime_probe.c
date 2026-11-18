/* SPDX-License-Identifier: 0BSD
 * Copyright (c) 2026 cdaunt. See windows_runtime.c for the complete license.
 * Exercise both the floating-point marker and native stack-probe support.
 */
__declspec(dllexport) double probe(double x) {
    volatile char buffer[32768];
    buffer[0] = (char)x;
    buffer[32767] = (char)(x + 1);
    return x * x + buffer[0] + buffer[32767];
}
