CC ?= clang
CFLAGS = -std=c11 -O2 -Wall -Wextra -Werror -D_DARWIN_C_SOURCE
OUT ?= build/prx-vmic
SRC = src/main.c src/hal.c src/io.c src/dsp.c src/wav.c src/selftest.c src/props.c

$(OUT): $(SRC) $(wildcard src/*.h)
	mkdir -p $(dir $(OUT))
	$(CC) $(CFLAGS) $(SRC) -o $(OUT) -framework CoreAudio -framework CoreFoundation -lpthread
	codesign --force --sign - $(OUT)

clean:
	rm -rf build
