/**
 * @file test_ffi.c
 * @brief Basic C tests for moca FFI
 *
 * This file tests the moca C API by exercising the core functions.
 * Compile with: gcc -o test_ffi test_ffi.c -L../../target/debug -lmoca -Wl,-rpath,../../target/debug
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>
#include "../../include/moca.h"

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
    const char *ver = moca_version();
    ASSERT_NOT_NULL(ver);
    ASSERT_EQ(strcmp(ver, "0.1.0"), 0);
    ASSERT_EQ(moca_version_major(), 0);
    ASSERT_EQ(moca_version_minor(), 1);
    ASSERT_EQ(moca_version_patch(), 0);
}

// =============================================================================
// VM Lifecycle Tests
// =============================================================================

TEST(vm_create_free) {
    MocaVm *vm = moca_vm_new();
    ASSERT_NOT_NULL(vm);
    moca_vm_free(vm);
}

TEST(vm_free_null) {
    // Should not crash
    moca_vm_free(NULL);
}

TEST(vm_has_chunk_initially_false) {
    MocaVm *vm = moca_vm_new();
    ASSERT(!moca_has_chunk(vm));
    moca_vm_free(vm);
}

// =============================================================================
// Stack Tests
// =============================================================================

TEST(stack_push_pop_i64) {
    MocaVm *vm = moca_vm_new();

    ASSERT_EQ(moca_get_top(vm), 0);

    moca_push_i64(vm, 42);
    ASSERT_EQ(moca_get_top(vm), 1);
    ASSERT(moca_is_i64(vm, -1));
    ASSERT_EQ(moca_to_i64(vm, -1), 42);

    moca_push_i64(vm, 123);
    ASSERT_EQ(moca_get_top(vm), 2);
    ASSERT_EQ(moca_to_i64(vm, -1), 123);
    ASSERT_EQ(moca_to_i64(vm, -2), 42);

    moca_pop(vm, 1);
    ASSERT_EQ(moca_get_top(vm), 1);
    ASSERT_EQ(moca_to_i64(vm, -1), 42);

    moca_vm_free(vm);
}

TEST(stack_push_pop_f64) {
    MocaVm *vm = moca_vm_new();

    moca_push_f64(vm, 3.14159);
    ASSERT(moca_is_f64(vm, -1));

    double val = moca_to_f64(vm, -1);
    ASSERT(val > 3.14 && val < 3.15);

    moca_vm_free(vm);
}

TEST(stack_push_pop_bool) {
    MocaVm *vm = moca_vm_new();

    moca_push_bool(vm, true);
    ASSERT(moca_is_bool(vm, -1));
    ASSERT(moca_to_bool(vm, -1) == true);

    moca_push_bool(vm, false);
    ASSERT(moca_to_bool(vm, -1) == false);

    moca_vm_free(vm);
}

TEST(stack_push_null) {
    MocaVm *vm = moca_vm_new();

    moca_push_null(vm);
    ASSERT(moca_is_null(vm, -1));

    moca_vm_free(vm);
}

TEST(stack_push_string) {
    MocaVm *vm = moca_vm_new();

    const char *str = "hello world";
    moca_push_string(vm, str, strlen(str));

    ASSERT(moca_is_string(vm, -1));

    size_t len = 0;
    const char *result = moca_to_string(vm, -1, &len);
    ASSERT_NOT_NULL(result);
    ASSERT_EQ(len, strlen(str));
    ASSERT_EQ(strncmp(result, str, len), 0);

    moca_vm_free(vm);
}

TEST(stack_set_top) {
    MocaVm *vm = moca_vm_new();

    moca_push_i64(vm, 1);
    moca_push_i64(vm, 2);
    moca_push_i64(vm, 3);
    ASSERT_EQ(moca_get_top(vm), 3);

    // Shrink
    moca_set_top(vm, 1);
    ASSERT_EQ(moca_get_top(vm), 1);
    ASSERT_EQ(moca_to_i64(vm, -1), 1);

    // Grow (pads with null)
    moca_set_top(vm, 3);
    ASSERT_EQ(moca_get_top(vm), 3);
    ASSERT(moca_is_null(vm, -1));
    ASSERT(moca_is_null(vm, -2));

    moca_vm_free(vm);
}

TEST(stack_negative_index) {
    MocaVm *vm = moca_vm_new();

    moca_push_i64(vm, 10);  // index 0, or -3
    moca_push_i64(vm, 20);  // index 1, or -2
    moca_push_i64(vm, 30);  // index 2, or -1

    ASSERT_EQ(moca_to_i64(vm, -1), 30);
    ASSERT_EQ(moca_to_i64(vm, -2), 20);
    ASSERT_EQ(moca_to_i64(vm, -3), 10);

    ASSERT_EQ(moca_to_i64(vm, 0), 10);
    ASSERT_EQ(moca_to_i64(vm, 1), 20);
    ASSERT_EQ(moca_to_i64(vm, 2), 30);

    moca_vm_free(vm);
}

// =============================================================================
// Error Tests
// =============================================================================

TEST(error_initially_none) {
    MocaVm *vm = moca_vm_new();

    ASSERT(!moca_has_error(vm));
    ASSERT_NULL(moca_get_error(vm));

    moca_vm_free(vm);
}

TEST(error_clear) {
    MocaVm *vm = moca_vm_new();

    // Try to call a non-existent function to trigger an error
    MocaResult res = moca_call(vm, "nonexistent", 0);
    ASSERT_EQ(res, MOCA_RESULT_ERROR_NOT_FOUND);
    ASSERT(moca_has_error(vm));

    moca_clear_error(vm);
    ASSERT(!moca_has_error(vm));
    ASSERT_NULL(moca_get_error(vm));

    moca_vm_free(vm);
}

// =============================================================================
// Globals Tests
// =============================================================================

TEST(globals_set_get) {
    MocaVm *vm = moca_vm_new();

    // Set a global
    moca_push_i64(vm, 42);
    MocaResult res = moca_set_global(vm, "my_var");
    ASSERT_EQ(res, MOCA_RESULT_OK);
    ASSERT_EQ(moca_get_top(vm), 0);  // Value should be consumed

    // Get it back
    res = moca_get_global(vm, "my_var");
    ASSERT_EQ(res, MOCA_RESULT_OK);
    ASSERT_EQ(moca_get_top(vm), 1);
    ASSERT_EQ(moca_to_i64(vm, -1), 42);

    moca_vm_free(vm);
}

TEST(globals_get_nonexistent) {
    MocaVm *vm = moca_vm_new();

    MocaResult res = moca_get_global(vm, "nonexistent");
    ASSERT_EQ(res, MOCA_RESULT_ERROR_NOT_FOUND);

    moca_vm_free(vm);
}

// =============================================================================
// Host Function Tests
// =============================================================================

static MocaResult host_add(MocaVm *vm) {
    int64_t a = moca_to_i64(vm, 0);
    int64_t b = moca_to_i64(vm, 1);
    moca_pop(vm, 2);
    moca_push_i64(vm, a + b);
    return MOCA_RESULT_OK;
}

TEST(host_function_register) {
    MocaVm *vm = moca_vm_new();

    MocaResult res = moca_register_function(vm, "add", host_add, 2);
    ASSERT_EQ(res, MOCA_RESULT_OK);

    // Function should be registered (can't call it without loaded bytecode though)

    moca_vm_free(vm);
}

// =============================================================================
// Bytecode Loading Tests
// =============================================================================

TEST(load_chunk_null) {
    MocaVm *vm = moca_vm_new();

    MocaResult res = moca_load_chunk(vm, NULL, 0);
    ASSERT_EQ(res, MOCA_RESULT_ERROR_INVALID_ARG);

    moca_vm_free(vm);
}

TEST(load_chunk_invalid) {
    MocaVm *vm = moca_vm_new();

    const uint8_t bad_data[] = "not valid bytecode";
    MocaResult res = moca_load_chunk(vm, bad_data, sizeof(bad_data));
    ASSERT_EQ(res, MOCA_RESULT_ERROR_VERIFY);

    moca_vm_free(vm);
}

TEST(load_file_not_found) {
    MocaVm *vm = moca_vm_new();

    MocaResult res = moca_load_file(vm, "/nonexistent/path");
    ASSERT_EQ(res, MOCA_RESULT_ERROR_NOT_FOUND);

    moca_vm_free(vm);
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
    MocaVm *vm = moca_vm_new();

    callback_called = 0;
    last_error_msg[0] = '\0';

    moca_set_error_callback(vm, error_callback, NULL);

    // Trigger an error
    MocaResult res = moca_call(vm, "nonexistent", 0);
    ASSERT_EQ(res, MOCA_RESULT_ERROR_NOT_FOUND);
    ASSERT(callback_called > 0);
    ASSERT(strlen(last_error_msg) > 0);

    moca_vm_free(vm);
}

// =============================================================================
// Main
// =============================================================================

int main(void) {
    printf("=== Moca FFI C Tests ===\n\n");

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
