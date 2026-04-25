#ifndef RSNAP_HOST_FFI_SHIM_H
#define RSNAP_HOST_FFI_SHIM_H

#include "../../../../../packages/rsnap-host-ffi/include/rsnap_host_ffi.h"

static inline uint32_t rsnap_status_code(enum RsnapStatus status) {
	return (uint32_t)status;
}

#endif
