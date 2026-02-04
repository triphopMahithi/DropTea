#include "wintoastlib.h"
#include <string>
#include <vector>
#include <iostream>
#include <shlobj.h>   
#include <propvarutil.h> 
#include <propkey.h>     

using namespace WinToastLib;

class MyHandler : public IWinToastHandler {
public:
    typedef void (*RustCallback)(int action_id);
    RustCallback callback;
    MyHandler(RustCallback cb) : callback(cb) {}

    void toastActivated() const override { if (callback) callback(0); }
    void toastActivated(int actionIndex) const override { if (callback) callback(actionIndex); }
    void toastActivated(std::wstring response) const override { if (callback) callback(0); }
    void toastDismissed(WinToastDismissalReason state) const override { if (callback) callback(-1); }
    void toastFailed() const override { if (callback) callback(-107); }
};

extern "C" {
    bool init_wintoast(const wchar_t* appName, const wchar_t* aumid) {
        if (!WinToast::isCompatible()) return false;
        WinToast::instance()->setAppName(appName);
        WinToast::instance()->setAppUserModelId(aumid);
        WinToast::WinToastError error;
        return WinToast::instance()->initialize(&error);
    }

    // ✅ เพิ่มฟังก์ชันนี้เพื่อให้ Rust เรียกสร้าง Shortcut ได้
    bool create_shortcut_native(const wchar_t* targetPath, const wchar_t* args, const wchar_t* workDir, const wchar_t* aumid, const wchar_t* shortcutName) {
        CoInitialize(NULL); 
        IShellLinkW* pShellLink = NULL; 
        HRESULT hres = CoCreateInstance(CLSID_ShellLink, NULL, CLSCTX_INPROC_SERVER, IID_IShellLinkW, (LPVOID*)&pShellLink);
        
        if (SUCCEEDED(hres)) {
            pShellLink->SetPath(targetPath);
            pShellLink->SetArguments(args);
            pShellLink->SetWorkingDirectory(workDir);
            pShellLink->SetDescription(L"DropTea File Transfer");

            IPropertyStore* pPropStore = NULL;
            hres = pShellLink->QueryInterface(IID_IPropertyStore, (LPVOID*)&pPropStore);
            if (SUCCEEDED(hres)) {
                PROPVARIANT pv;
                if (SUCCEEDED(InitPropVariantFromString(aumid, &pv))) {
                    pPropStore->SetValue(PKEY_AppUserModel_ID, pv);
                    pPropStore->Commit();
                    PropVariantClear(&pv);
                }
                pPropStore->Release();
            }

            IPersistFile* pPersistFile = NULL;
            hres = pShellLink->QueryInterface(IID_IPersistFile, (LPVOID*)&pPersistFile);
            if (SUCCEEDED(hres)) {
                wchar_t path[MAX_PATH];
                if (SUCCEEDED(SHGetFolderPathW(NULL, CSIDL_PROGRAMS, NULL, 0, path))) {
                    std::wstring fullPath = std::wstring(path) + L"\\" + shortcutName + L".lnk";
                    hres = pPersistFile->Save(fullPath.c_str(), TRUE);
                }
                pPersistFile->Release();
            }
            pShellLink->Release();
        }
        CoUninitialize();
        return SUCCEEDED(hres);
    }

    void show_request_toast(const wchar_t* title, const wchar_t* msg, const wchar_t* imagePath, void (*rust_cb)(int)) {
        if (!rust_cb) return;
        try {
            WinToastTemplate templ = WinToastTemplate(WinToastTemplate::ImageAndText02);
            templ.setTextField(title, WinToastTemplate::FirstLine);
            templ.setTextField(msg, WinToastTemplate::SecondLine);
            templ.setExpiration(45000); 
            templ.addAction(L"Accept");
            templ.addAction(L"Decline");

            if (imagePath && wcslen(imagePath) > 0) {
                templ.setImagePath(imagePath);
            }

            WinToast::WinToastError error = WinToast::WinToastError::NoError;
            INT64 id = WinToast::instance()->showToast(templ, new MyHandler(rust_cb), &error);
            if (id < 0) rust_cb(-100 - (int)error);
        } catch (...) {
            rust_cb(-108);
        }
    }

    void show_info_toast(const wchar_t* title, const wchar_t* msg, const wchar_t* imagePath) {
        if (!WinToast::isCompatible()) return;
        WinToastTemplate templ = WinToastTemplate(WinToastTemplate::ImageAndText02);
        templ.setTextField(title, WinToastTemplate::FirstLine);
        templ.setTextField(msg, WinToastTemplate::SecondLine);
        templ.setExpiration(5000); 
        if (imagePath && wcslen(imagePath) > 0) templ.setImagePath(imagePath);
        WinToast::instance()->showToast(templ, new MyHandler(nullptr)); 
    }
}