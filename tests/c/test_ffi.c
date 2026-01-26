/**
 * @file test_ffi.c
 * @brief Basic C tests for mica FFI
 *
 * This file tests the mica C API by exercising the core functions.
 * Compile with: gcc -o test_ffi test_ffi.c -L../../target/debug -lmica -Wl,-rpath,../../target/debug
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include "../../include/mica.h"

// Test counter
static int tests_passed = 0;
static int tests_failed = 0;

#define TEST(name) void test_##name(void)
#define RUN_TEST(name) do { \
    printf("Running " #name "... "); \
    test_##name(); \
    tests_passed++; \
    printf("OK\n"); \
} while(0)

#define ASSERT(cond) do { \
    if (!(cond)) { \
        printf("FAILED: %s at %s:%d\n", #cond, __FILE__, __LINE__); \
        tests_failed++; \
        return; \
    } \
} while(0)

#define ASSERT_EQ(a, b) ASSERT((a) == (b))
#define ASSERT_NE(a, b) ASSERT((a) != (b))
#define ASSERT_NULL(a) ASSERT((a) == NULL)
#define ASSERT_NOT_NULL(a) ASSERT((a) != NULL)

// =============================================================================
// Version Tests
// =============================================================================

TEST(version) {
    const char *ver = mica_version();
    ASSERT_NOT_NULL(ver);
    ASSERT_EQ(strcmp(ver, "0.1.0"), 0);
    ASSERT_EQ(mica_version_major(), 0);
    ASSERT_EQ(mica_version_minor(), 1);
    ASSERT_EQ(mica_version_patch(), 0);
}

// =============================================================================
// VM Lifecycle Tests
// =============================================================================

TEST(vm_create_free) {
    MicaVm *vm = mica_vm_new();
    ASSERT_NOT_NULL(vm);
    mica_vm_free(vm);
}

TEST(vm_free_null) {
    // Should not crash
    mica_vm_free(NULL);
}

TEST(vm_has_chunk_initially_false) {
    MicaVm *vm = mica_vm_new();
    ASSERT(!mica_has_chunk(vm));
    mica_vm_free(vm);
}

// =============================================================================
// Stack Tests
// =============================================================================

TEST(stack_push_pop_i64) {
    MicaVm *vm = mica_vm_new();

    ASSERT_EQ(mica_get_top(vm), 0);

    mica_push_i64(vm, 42);
    ASSERT_EQ(mica_get_top(vm), 1);
    ASSERT(mica_is_i64(vm, -1));
    ASSERT_EQ(mica_to_i64(vm, -1), 42);

    mica_push_i64(vm, 123);
    ASSERT_EQ(mica_get_top(vm), 2);
    ASSERT_EQ(mica_to_i64(vm, -1), 123);
    ASSERT_EQ(mica_to_i64(vm, -2), 42);

    mica_pop(vm, 1);
    ASSERT_EQ(mica_get_top(vm), 1);
    ASSERT_EQ(mica_to_i64(vm, -1), 42);

    mica_vm_free(vm);
}

TEST(stack_push_pop_f64) {
    MicaVm *vm = mica_vm_new();

    mica_push_f64(vm, 3.14159);
    ASSERT(mica_is_f64(vm, -1));

    double val = mica_to_f64(vm, -1);
    ASSERT(val > 3.14 && val < 3.15);

    mica_vm_free(vm);
}

TEST(stack_push_pop_bool) {
    MicaVm *vm = mica_vm_new();

    mica_push_bool(vm, true);
    ASSERT(mica_is_bool(vm, -1));
    ASSERT(mica_to_bool(vm, -1) == true);

    mica_push_bool(vm, false);
    ASSERT(mica_to_bool(vm, -1) == false);

    mica_vm_free(vm);
}

TEST(stack_push_null) {
    MicaVm *vm = mica_vm_new();

    mica_push_null(vm);
    ASSERT(mica_is_null(vm, -1));

    mica_vm_free(vm);
}

TEST(stack_push_string) {
    MicaVm *vm = mica_vm_new();

    const char *str = "hello world";
    mica_push_string(vm, str, strlen(str));

    ASSERT(mica_is_string(vm, -1));

    size_t len = 0;
    const char *result = mica_to_string(vm, -1, &len);
    ASSERT_NOT_NULL(result);
    ASSERT_EQ(len, strlen(str));
    ASSERT_EQ(strncmp(result, str, len), 0);

    mica_vm_free(vm);
}

TEST(stack_set_top) {
    MicaVm *vm = mica_vm_new();

    mica_push_i64(vm, 1);
    mica_push_i64(vm, 2);
    mica_push_i64(vm, 3);
    ASSERT_EQ(mica_get_top(vm), 3);

    // Shrink
    mica_set_top(vm, 1);
    ASSERT_EQ(mica_get_top(vm), 1);
    ASSERT_EQ(mica_to_i64(vm, -1), 1);

    // Grow (pads with null)
    mica_set_top(vm, 3);
    ASSERT_EQ(mica_get_top(vm), 3);
    ASSERT(mica_is_null(vm, -1));
    ASSERT(mica_is_null(vm, -2));

    mica_vm_free(vm);
}

TEST(stack_negative_index) {
    MicaVm *vm = mica_vm_new();

    mica_push_i64(vm, 10);  // index 0, or -3
    mica_push_i64(vm, 20);  // index 1, or -2
    mica_push_i64(vm, 30);  // index 2, or -1

    ASSERT_EQ(mica_to_i64(vm, -1), 30);
    ASSERT_EQ(mica_to_i64(vm, -2), 20);
    ASSERT_EQ(mica_to_i64(vm, -3), 10);

    ASSERT_EQ(mica_to_i64(vm, 0), 10);
    ASSERT_EQ(mica_to_i64(vm, 1), 20);
    ASSERT_EQ(mica_to_i64(vm, 2), 30);

    mica_vm_free(vm);
}

// =============================================================================
// Error Tests
// =============================================================================

TEST(error_initially_none) {
    MicaVm *vm = mica_vm_new();

    ASSERT(!mica_has_error(vm));
    ASSERT_NULL(mica_get_error(vm));

    mica_vm_free(vm);
}

TEST(error_clear) {
    MicaVm *vm = mica_vm_new();

    // Try to call a non-existent function to trigger an error
    MicaResult res = mica_call(vm, "nonexistent", 0);
    ASSERT_EQ(res, MICA_RESULT_ERROR_NOT_FOUND);
    ASSERT(mica_has_error(vm));

    mica_clear_error(vm);
    ASSERT(!mica_has_error(vm));
    ASSERT_NULL(mica_get_error(vm));

    mica_vm_free(vm);
}

// =============================================================================
// Globals Tests
// =============================================================================

TEST(globals_set_get) {
    MicaVm *vm = mica_vm_new();

    // Set a global
    mica_push_i64(vm, 42);
    MicaResult res = mica_set_global(vm, "my_var");
    ASSERT_EQ(res, MICA_RESULT_OK);
    ASSERT_EQ(mica_get_top(vm), 0);  // Value should be consumed

    // Get it back
    res = mica_get_global(vm, "my_var");
    ASSERT_EQ(res, MICA_RESULT_OK);
    ASSERT_EQ(mica_get_top(vm), 1);
    ASSERT_EQ(mica_to_i64(vm, -1), 42);

    mica_vm_free(vm);
}

TEST(globals_get_nonexistent) {
    MicaVm *vm = mica_vm_new();

    MicaResult res = mica_get_global(vm, "nonexistent");
    ASSERT_EQ(res, MICA_RESULT_ERROR_NOT_FOUND);

    mica_vm_free(vm);
}

// =============================================================================
// Host Function Tests
// =============================================================================

static MicaResult host_add(MicaVm *vm) {
    int64_t a = mica_to_i64(vm, 0);
    int64_t b = mica_to_i64(vm, 1);
    mica_pop(vm, 2);
    mica_push_i64(vm, a + b);
    return MICA_RESULT_OK;
}

TEST(host_function_register) {
    MicaVm *vm = mica_vm_new();

    MicaResult res = mica_register_function(vm, "add", host_add, 2);
    ASSERT_EQ(res, MICA_RESULT_OK);

    // Function should be registered (can't call it without loaded bytecode though)

    mica_vm_free(vm);
}

// =============================================================================
// Bytecode Loading Tests
// =============================================================================

TEST(load_chunk_null) {
    MicaVm *vm = mica_vm_new();

    MicaResult res = mica_load_chunk(vm, NULL, 0);
    ASSERT_EQ(res, MICA_RESULT_ERROR_INVALID_ARG);

    mica_vm_free(vm);
}

TEST(load_chunk_invalid) {
    MicaVm *vm = mica_vm_new();

    const uint8_t bad_data[] = "not valid bytecode";
    MicaResult res = mica_load_chunk(vm, bad_data, sizeof(bad_data));
    ASSERT_EQ(res, MICA_RESULT_ERROR_VERIFY);

    mica_vm_free(vm);
}

TEST(load_file_not_found) {
    MicaVm *vm = mica_vm_new();

    MicaResult res = mica_load_file(vm, "/nonexistent/path");
    ASSERT_EQ(res, MICA_RESULT_ERROR_NOT_FOUND);

    mica_vm_free(vm);
}

// =============================================================================
// Error Callback Test
// =============================================================================

static int callback_called = 0;
static char last_error_msg[256] = {0};

static void error_callback(const char *message, void *userdata) {
    (void)userdata;
    callback_called++;
    if (message) {
        strncpy(last_error_msg, message, sizeof(last_error_msg) - 1);
    }
}

TEST(error_callback) {
    MicaVm *vm = mica_vm_new();

    callback_called = 0;
    last_error_msg[0] = '\0';

    mica_set_error_callback(vm, error_callback, NULL);

    // Trigger an error
    MicaResult res = mica_call(vm, "nonexistent", 0);
    ASSERT_EQ(res, MICA_RESULT_ERROR_NOT_FOUND);
    ASSERT(callback_called > 0);
    ASSERT(strlen(last_error_msg) > 0);

    mica_vm_free(vm);
}

// =============================================================================
// Main
// =============================================================================

int main(void) {
    printf("=== Mica FFI C Tests ===\n\n");

    // Version tests
    RUN_TEST(version);

    // VM lifecycle tests
    RUN_TEST(vm_create_free);
    RUN_TEST(vm_free_null);
    RUN_TEST(vm_has_chunk_initially_false);

    // Stack tests
    RUN_TEST(stack_push_pop_i64);
    RUN_TEST(stack_push_pop_f64);
    RUN_TEST(stack_push_pop_bool);
    RUN_TEST(stack_push_null);
    RUN_TEST(stack_push_string);
    RUN_TEST(stack_set_top);
    RUN_TEST(stack_negative_index);

    // Error tests
    RUN_TEST(error_initially_none);
    RUN_TEST(error_clear);
    RUN_TEST(error_callback);

    // Globals tests
    RUN_TEST(globals_set_get);
    RUN_TEST(globals_get_nonexistent);

    // Host function tests
    RUN_TEST(host_function_register);

    // Bytecode loading tests
    RUN_TEST(load_chunk_null);
    RUN_TEST(load_chunk_invalid);
    RUN_TEST(load_file_not_found);

    printf("\n=== Results: %d passed, %d failed ===\n", tests_passed, tests_failed);

    return tests_failed > 0 ? 1 : 0;
}
