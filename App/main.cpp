#include <iostream>
#include <string>
#include <vector>
#include <sstream>
#include <windows.h>
#include <shlobj.h> // สำหรับหา path Downloads
#include <algorithm> // สำหรับ transform (tolower)

#include "droptea_api.h"
#include "wintoastlib.h"

using namespace WinToastLib;
using namespace std;

DropTeaHandle global_core = nullptr;

// ================= Helper Functions =================
std::wstring to_wstring(const std::string& str) {
    if (str.empty()) return std::wstring();
    int size_needed = MultiByteToWideChar(CP_UTF8, 0, &str[0], (int)str.size(), NULL, 0);
    std::wstring wstrTo(size_needed, 0);
    MultiByteToWideChar(CP_UTF8, 0, &str[0], (int)str.size(), &wstrTo[0], size_needed);
    return wstrTo;
}

std::string get_computer_name() {
    char buffer[MAX_COMPUTERNAME_LENGTH + 1];
    DWORD size = MAX_COMPUTERNAME_LENGTH + 1;
    if (GetComputerNameA(buffer, &size)) return std::string(buffer);
    return "Unknown-Device";
}

std::string get_downloads_path() {
    PWSTR path = NULL;
    if (SUCCEEDED(SHGetKnownFolderPath(FOLDERID_Downloads, 0, NULL, &path))) {
        int size_needed = WideCharToMultiByte(CP_UTF8, 0, path, -1, NULL, 0, NULL, NULL);
        std::string strTo(size_needed, 0);
        WideCharToMultiByte(CP_UTF8, 0, path, -1, &strTo[0], size_needed, NULL, NULL);
        CoTaskMemFree(path);
        if (!strTo.empty() && strTo.back() == '\0') strTo.pop_back();
        return strTo;
    }
    return "./downloads"; 
}

// สร้าง Shortcut อัตโนมัติ: จำเป็นสำหรับ Windows 10/11 Notifications
void setup_shortcut(const std::wstring& aumid, const std::wstring& appName) {
    wchar_t exePath[MAX_PATH];
    GetModuleFileNameW(NULL, exePath, MAX_PATH);
    create_shortcut_native(exePath, L"", L"", aumid.c_str(), appName.c_str());
}

// ================= Toast Handler =================
class RequestToastHandler : public IWinToastHandler {
    std::string _taskId;
public:
    RequestToastHandler(std::string taskId) : _taskId(taskId) {}
    
    void toastActivated(int actionIndex) const override {
        bool accepted = (actionIndex == 0); 
        std::cout << "[UI] Action: " << (accepted ? "ACCEPT" : "DECLINE") << std::endl;
        if (global_core) droptea_resolve_request(global_core, _taskId.c_str(), accepted);
    }
    
    void toastActivated() const override {
        std::cout << "[UI] User clicked toast body" << std::endl;
    }
    
    void toastActivated(std::wstring) const override {
        // Not used
    }
    
    void toastDismissed(WinToastDismissalReason) const override {
        std::cout << "[UI] Toast dismissed/timeout" << std::endl;
        if (global_core) droptea_resolve_request(global_core, _taskId.c_str(), false);
    }
    
    void toastFailed() const override {
        std::cout << "[UI] Toast failed to show" << std::endl;
        if (global_core) droptea_resolve_request(global_core, _taskId.c_str(), false);
    }
};

// ================= Rust Callback =================
void on_rust_event(int type, const char* task_id, const char* d1, const char* d2, uint64_t v1, uint64_t v2) {
    std::string id = task_id ? task_id : "";
    std::string data1 = d1 ? d1 : "";
    std::string data2 = d2 ? d2 : "";

    switch (type) {
        case 0: // Log
            std::cout << "[Rust Log] " << data1 << std::endl;
            break;
            
        case 1: // Peer Found
            std::cout << "[Discovery] Found: " << data1 << " (" << data1 << ")" << std::endl;
            break;
            
        case 3: // Progress
            if (v2 > 0 && (v1 % (v2/10) == 0)) 
                std::cout << "[Transfer] " << id << ": " << (v1 * 100 / v2) << "%" << std::endl;
            break;
            
        case 4: // Completed
            std::cout << "[Transfer] Completed: " << data1 << std::endl;
            {
                WinToastTemplate templ = WinToastTemplate(WinToastTemplate::Text02);
                templ.setTextField(L"File Transfer Complete", WinToastTemplate::FirstLine);
                templ.setTextField(to_wstring("Saved to: " + data1), WinToastTemplate::SecondLine);
                WinToast::instance()->showToast(templ, new RequestToastHandler(""));
            }
            break;
            
        case 5: // Error
            std::cerr << "[Error] " << id << ": " << data1 << std::endl;
            break;
            
        case 6: // Incoming Request
        {
            std::cout << "[Request] Incoming from " << data1 << std::endl;
            
            // Format: [[REQUEST]]|filename|size|sender|device
            std::string filename = "Unknown File";
            std::string sender = "Unknown Sender";
            
            size_t first_pipe = data1.find('|');
            if (first_pipe != std::string::npos) {
                 filename = data1.substr(first_pipe + 1);
                 size_t second_pipe = filename.find('|');
                 if (second_pipe != std::string::npos) {
                     filename = filename.substr(0, second_pipe);
                 }
            }

            WinToastTemplate templ = WinToastTemplate(WinToastTemplate::ImageAndText02);
            templ.setTextField(L"Incoming File Request", WinToastTemplate::FirstLine);
            templ.setTextField(to_wstring("File: " + filename), WinToastTemplate::SecondLine);
            templ.addAction(L"Accept");
            templ.addAction(L"Decline");
            templ.setExpiration(30000); 
            WinToast::instance()->showToast(templ, new RequestToastHandler(id));
            break;
        }
        
        case 10: // Server Started
            std::cout << "[System] Server listening on port: " << data1 << std::endl;
            break;
            
        default:
            break;
    }
}

// ================= Main =================
int main(int argc, char* argv[]) {
    int port = 8080;
    int mode = 0; // 0 = TCP (Default), 1 = QUIC, 2 = PlainTCP
    
    // Parse arguments: app.exe [port] [mode]
    if (argc > 1) port = std::stoi(argv[1]);
    
    if (argc > 2) {
        std::string m = argv[2];
        // แปลงเป็นตัวพิมพ์เล็ก
        std::transform(m.begin(), m.end(), m.begin(), [](unsigned char c){ return std::tolower(c); });

        if (m == "quic") {
            mode = 1;
        } else if (m == "plain" || m == "plaintcp") {
            mode = 2; // 🟢 Plain TCP Mode
        }
    }

    std::string device_name = get_computer_name();
    std::string download_path = get_downloads_path();
    
    // Config & Create Shortcut
    const std::wstring APP_NAME = L"DropTea Host";
    const std::wstring AUMID = L"DropTea.Core.Cpp"; 

    setup_shortcut(AUMID, APP_NAME);

    // Init WinToast
    WinToast::instance()->setAppName(APP_NAME);
    WinToast::instance()->setAppUserModelId(AUMID);
    if (!WinToast::instance()->initialize()) {
        std::cerr << "Warning: WinToast failed to initialize." << std::endl;
    }
    
    std::string modeStr;
    switch(mode) {
        case 1: modeStr = "QUIC (UDP)"; break;
        case 2: modeStr = "Plain TCP (No TLS)"; break;
        default: modeStr = "TCP (TLS)"; break;
    }

    std::cout << "---------------------------------------" << std::endl;
    std::cout << " Device Name : " << device_name << std::endl;
    std::cout << " Storage     : " << download_path << std::endl;
    std::cout << " Port        : " << port << std::endl;
    std::cout << " Mode        : " << modeStr << std::endl;
    std::cout << "---------------------------------------" << std::endl;

    // Init Rust (ส่ง Port, Mode และ Path เข้าไป)
    global_core = droptea_init(download_path.c_str(), (uint16_t)port, mode, on_rust_event);

    if (global_core) {
        // Start Service
        droptea_start_service(global_core, (uint16_t)port, device_name.c_str(), true);
        
        std::cout << "Server is running. Press Ctrl+C to exit." << std::endl;
        
        MSG msg;
        while (GetMessage(&msg, NULL, 0, 0)) {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
        }
        droptea_free(global_core);
    } else {
        std::cerr << "Failed to init Rust core" << std::endl;
        return 1;
    }

    return 0;
}