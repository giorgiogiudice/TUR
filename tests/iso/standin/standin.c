// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for Windows Boot Manager, for the "beside Windows on the same
// disk" tests of the installer medium (tests/iso, ADR-0028). It is NOT a
// Microsoft program and contains no Microsoft code: a few lines of TUR's own
// that a test disk carries at EFI/Microsoft/Boot/bootmgfw.efi, where Windows
// keeps its boot manager.
//
// When the firmware or TUR's boot menu starts it, it says so on the console
// (which OVMF also sends to the serial port), with the file it was loaded
// from and the device path of the partition, then powers the machine off.
// A test reads that line to tell which file on which partition was started.
//
// Built in CI only (tests/iso/build-standin.sh, gnu-efi), never shipped.
#include <efi.h>
#include <efilib.h>

EFI_STATUS EFIAPI efi_main(EFI_HANDLE image, EFI_SYSTEM_TABLE *system)
{
    EFI_LOADED_IMAGE *loaded = NULL;
    CHAR16 *file = L"unknown";
    CHAR16 *device = L"unknown";

    InitializeLib(image, system);
    if (uefi_call_wrapper(BS->HandleProtocol, 3, image, &LoadedImageProtocol, (void **)&loaded) == EFI_SUCCESS
        && loaded != NULL) {
        if (loaded->FilePath != NULL)
            file = DevicePathToStr(loaded->FilePath);
        EFI_DEVICE_PATH *path = DevicePathFromHandle(loaded->DeviceHandle);
        if (path != NULL)
            device = DevicePathToStr(path);
    }
    Print(L"\r\nTUR-WINDOWS-STANDIN: started file=%s\r\n", file);
    Print(L"TUR-WINDOWS-STANDIN: device=%s\r\n", device);
    Print(L"TUR-WINDOWS-STANDIN: this is TUR's test program, not Windows; powering off\r\n");
    uefi_call_wrapper(BS->Stall, 1, 3000000);
    uefi_call_wrapper(RT->ResetSystem, 4, EfiResetShutdown, EFI_SUCCESS, 0, NULL);
    return EFI_SUCCESS;
}
