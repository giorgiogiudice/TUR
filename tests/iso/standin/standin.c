// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0
//
// A stand-in for Windows Boot Manager, for the "beside Windows on the same
// disk" tests of the installer medium (tests/iso, ADR-0029). It is NOT a
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

// Called by gnu-efi's start code with the System V convention: not EFIAPI.
EFI_STATUS efi_main(EFI_HANDLE image, EFI_SYSTEM_TABLE *system)
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
    // The partition it came from, on a short line of its own (a console
    // line is 80 characters; the whole device path is longer).
    if (loaded != NULL) {
        EFI_DEVICE_PATH *node = DevicePathFromHandle(loaded->DeviceHandle);
        while (node != NULL && !IsDevicePathEnd(node)) {
            if (DevicePathType(node) == MEDIA_DEVICE_PATH && DevicePathSubType(node) == MEDIA_HARDDRIVE_DP) {
                HARDDRIVE_DEVICE_PATH *hd = (HARDDRIVE_DEVICE_PATH *)node;
                if (hd->SignatureType == SIGNATURE_TYPE_GUID)
                    Print(L"TUR-WINDOWS-STANDIN: partition=%d guid=%g\r\n", hd->PartitionNumber,
                          (EFI_GUID *)hd->Signature);
            }
            node = NextDevicePathNode(node);
        }
    }
    Print(L"TUR-WINDOWS-STANDIN: device=%s\r\n", device);
    Print(L"TUR-WINDOWS-STANDIN: this is TUR's test program, not Windows; powering off\r\n");
    uefi_call_wrapper(BS->Stall, 1, 3000000);
    uefi_call_wrapper(RT->ResetSystem, 4, EfiResetShutdown, EFI_SUCCESS, 0, NULL);
    return EFI_SUCCESS;
}
