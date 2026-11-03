// clang handles the SDK vector ABI; Rust crosses this boundary with byte pointers.
#include <Hypervisor/Hypervisor.h>
#include <string.h>

hv_return_t hopper_simd_get(hv_vcpu_t cpu, uint32_t reg, uint8_t *bytes) {
  hv_simd_fp_uchar16_t value;
  hv_return_t result = hv_vcpu_get_simd_fp_reg(cpu, reg, &value);
  if (result == HV_SUCCESS) memcpy(bytes, &value, sizeof(value));
  return result;
}

hv_return_t hopper_simd_set(hv_vcpu_t cpu, uint32_t reg, const uint8_t *bytes) {
  hv_simd_fp_uchar16_t value;
  memcpy(&value, bytes, sizeof(value));
  return hv_vcpu_set_simd_fp_reg(cpu, reg, value);
}
