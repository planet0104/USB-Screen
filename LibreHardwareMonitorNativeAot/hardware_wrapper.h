#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct NativeBuffer {
    uint8_t* ptr;
    size_t len;
} NativeBuffer;

int32_t lhm_init(void);
int32_t lhm_update(void);
NativeBuffer lhm_get_json(void);
NativeBuffer lhm_get_all_sensors_json(void);
NativeBuffer lhm_get_last_error(void);
void lhm_free_buffer(uint8_t* ptr, size_t len);
void lhm_close(void);

#ifdef __cplusplus
}
#endif
