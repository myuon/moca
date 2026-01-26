# Makefile for moca C FFI tests

CC = gcc
CFLAGS = -Wall -Wextra -std=c11 -g
LDFLAGS = -L../../target/release -lmoca -Wl,-rpath,../../target/release -lpthread -ldl -lm

# For debug builds
LDFLAGS_DEBUG = -L../../target/debug -lmoca -Wl,-rpath,../../target/debug -lpthread -ldl -lm

INCLUDES = -I../../include

TESTS = test_ffi

.PHONY: all clean test build-lib build-lib-debug

all: build-lib $(TESTS)

debug: build-lib-debug $(TESTS:=_debug)

build-lib:
	cd ../.. && cargo build --release

build-lib-debug:
	cd ../.. && cargo build

test_ffi: test_ffi.c ../../include/moca.h
	$(CC) $(CFLAGS) $(INCLUDES) -o $@ $< $(LDFLAGS)

test_ffi_debug: test_ffi.c ../../include/moca.h
	$(CC) $(CFLAGS) $(INCLUDES) -o $@ $< $(LDFLAGS_DEBUG)

test: all
	./test_ffi

test-debug: debug
	./test_ffi_debug

clean:
	rm -f $(TESTS) $(TESTS:=_debug) *.o
