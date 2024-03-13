CC=gcc
CFLAGS=-Wall -g -O0

SOURCE_FILES=./src/plugin.c ./src/source.c ./src/output.c
INCLUDE_DIRS=../alumet/generated
INC_PARAMS=$(addprefix -I, $(INCLUDE_DIRS))

# flags that must be there to compile as a shared library, you should NOT change them
DYLIB_FLAGS=-shared -fvisibility=hidden -fPIC

plugin:
	mkdir -p target
	$(CC) $(CFLAGS) $(DYLIB_FLAGS) -o ./target/plugin.so $(INC_PARAMS) $(SOURCE_FILES)
