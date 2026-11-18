"""Exercise Windows runtime support in an isolated, short-lived process."""

import ctypes
import sys

if __name__ == "__main__":
    # Process exit releases the DLL even if a check fails.
    library = ctypes.CDLL(sys.argv[1])
    library.probe.argtypes = [ctypes.c_double]
    library.probe.restype = ctypes.c_double
    if library.probe(2.0) != 9.0:
        raise RuntimeError("Windows floating-point/stack-probe support failed")
