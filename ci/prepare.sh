#!/bin/bash

# 检查TARGET环境变量
if [[ -z "${TARGET}" ]]; then
    echo "TARGET not set. Exiting."
    exit 1
fi

# 编译动态库
compile_dylib() {
    local target=$1
    local dylib_files=("liba.rs" "libb.rs" "libc.rs")
    
    for name in "${dylib_files[@]}"; do
        echo "Compiling dylib: $name"
        rustc -O --target "$target" -C panic=abort -C linker=rust-lld \
            "test-dylib/$name" --out-dir target || {
                echo "Could not compile the dylibs!"
                exit 1
            }
    done
}

# 编译可重定位对象文件
compile_relocatable() {
    local target=$1
    local dylib_files=("liba.rs" "libb.rs" "libc.rs")
    
    for name in "${dylib_files[@]}"; do
        echo "Compiling relocatable object: $name"
        rustc -O --target "$target" --emit obj -C panic=abort -C linker=rust-lld \
            "test-dylib/$name" --out-dir target || {
                echo "Could not compile the relocatable object!"
                exit 1
            }
    done

}

# 编译可执行文件
compile_exec() {
    local target=$1
    local exec_files=("exec_a.c")
    
    # 根据目标架构选择编译器
    local compiler=""
    if [[ $target == riscv* ]]; then
        compiler="riscv64-linux-gnu-gcc"
    elif [[ $target == aarch64* ]]; then
        compiler="aarch64-linux-gnu-gcc"
    elif [[ $target == x86_64* ]]; then
        compiler="x86_64-linux-gnu-gcc"
    else
        echo "Unsupported target: $target"
        return
    fi
    
    for name in "${exec_files[@]}"; do
        local source_file="test-exec/$name"
        local output_name="${name%.*}"  # 移除文件扩展名
        local output_path="target/$output_name"
        
        echo "Compiling executable: $name"
        $compiler -O2 -static "$source_file" -o "$output_path" || {
            echo "Could not compile the executables!"
            exit 1
        }
    done
}

compile_dylib "$TARGET"
compile_exec "$TARGET"
compile_relocatable "$TARGET"