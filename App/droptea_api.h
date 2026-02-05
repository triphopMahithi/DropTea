#pragma once
#include <cstdint>

// Handle สำหรับ Rust Context
typedef void* DropTeaHandle;

// Callback type ต้องตรงกับ Rust: (type, task_id, data1, data2, val1, val2)
typedef void (*RustCallback)(int, const char*, const char*, const char*, uint64_t, uint64_t);

extern "C" {
    // เพิ่ม parameter port (uint16_t)
    DropTeaHandle droptea_init(const char* storage_path, uint16_t port, int mode, RustCallback callback);
    
    void droptea_start_service(DropTeaHandle ctx, uint16_t port, const char* device_id, bool dev_mode);
    void droptea_resolve_request(DropTeaHandle ctx, const char* task_id, bool accept);
    void droptea_free(DropTeaHandle ctx);
    
    // ฟังก์ชันจาก bridge.cpp สำหรับสร้าง Shortcut
    bool create_shortcut_native(const wchar_t* targetPath, const wchar_t* args, const wchar_t* workDir, const wchar_t* aumid, const wchar_t* shortcutName);
}