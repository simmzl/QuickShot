use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use windows::core::{Interface, GUID, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{
    NotificationSetting, ToastFailedEventArgs, ToastNotification, ToastNotificationManager,
};
use windows_registry::{Key, CURRENT_USER};
use windows_sys::Win32::Storage::FileSystem::DELETE;

const APP_ID: &str = "simmzl.QuickShot";
const SCHEME: &str = "quickshot-view:";
const COPIED: &str = "quickshot-view:copied";
const FILES: &str = r"Software\QuickShot\NotificationFiles";

const INSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\QuickShot";
const PROTOCOL: &str = r"Software\Classes\quickshot-view";

const ACTIVATOR: GUID = GUID::from_u128(0x93c8c63f_50ab_4e16_a0b5_381ac09ed40f);

pub struct NotificationRuntime;

impl NotificationRuntime {
    pub fn initialize() -> Result<Self> {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_SINGLETHREADED};
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.context("initialize Windows Runtime")?;
        Ok(Self)
    }
}

impl Drop for NotificationRuntime {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::WinRT::RoUninitialize() };
    }
}

pub fn log_failure(message: &str) {
    eprintln!("Windows notification: {message}");
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("QuickShot");
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(
                dir.join("notification-error.log"),
                format!("{} UTC  {message}\n", time::OffsetDateTime::now_utc()),
            );
        }
    }
}

pub fn prepare() -> Result<()> {
    // Cache only successful registration. A later capture retries a failed startup.
    static READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if READY.get().is_some() {
        return Ok(());
    }
    unsafe {
        windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_ID))?;
    }
    let identity = CURRENT_USER.create(r"Software\Classes\AppUserModelId\simmzl.QuickShot")?;
    identity.set_string("DisplayName", "QuickShot")?;
    // A stub CLSID permits persistent protocol-activated desktop toasts.
    identity.set_string("CustomActivator", format!("{{{ACTIVATOR:?}}}"))?;
    let current_exe = std::env::current_exe()?;
    if let Err(error) = register_protocol(CURRENT_USER, &current_exe) {
        log_failure(&format!("register notification click handler: {error:#}"));
    }
    let exe = handler_executable(CURRENT_USER, &current_exe);
    ensure_shortcut(&programs_folder()?.join("QuickShot.lnk"), &exe)
        .context("register QuickShot in the Start menu")?;
    let _ = READY.set(());
    Ok(())
}

fn programs_folder() -> Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Programs, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None)?;
        let folder = std::ffi::OsString::from_wide(path.as_wide());
        CoTaskMemFree(Some(path.0.cast()));
        Ok(folder.into())
    }
}

fn ensure_shortcut(path: &Path, exe: &Path) -> Result<()> {
    use windows::Win32::{
        Storage::EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_AppUserModel_ToastActivatorCLSID},
        System::Com::{
            CoCreateInstance, IPersistFile,
            StructuredStorage::{InitPropVariantFromCLSID, PROPVARIANT},
            CLSCTX_INPROC_SERVER, STGM_READWRITE,
        },
        UI::Shell::{
            IShellLinkW,
            PropertiesSystem::{IPropertyStore, PSCoerceToCanonicalValue},
            SHChangeNotify, ShellLink, SHCNE_CREATE, SHCNE_UPDATEITEM, SHCNF_FLUSH, SHCNF_PATHW,
        },
    };

    let filename = HSTRING::from(path);
    let target = HSTRING::from(exe);
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = link.cast()?;
        let exists = path.exists();
        let loaded = exists && file.Load(&filename, STGM_READWRITE).is_ok();
        let props: IPropertyStore = link.cast()?;
        let mut id = PROPVARIANT::from(APP_ID);
        PSCoerceToCanonicalValue(&PKEY_AppUserModel_ID, &mut id)?;
        let activator = InitPropVariantFromCLSID(&ACTIVATOR)?;
        if loaded {
            let mut old_target = vec![0u16; 32768];
            if link
                .GetPath(&mut old_target, std::ptr::null_mut(), 0)
                .is_ok()
                && old_target.split(|c| *c == 0).next() == Some(&target[..])
                && props.GetValue(&PKEY_AppUserModel_ID).is_ok_and(|v| v == id)
                && props
                    .GetValue(&PKEY_AppUserModel_ToastActivatorCLSID)
                    .is_ok_and(|v| v == activator)
            {
                return Ok(());
            }
        }
        link.SetPath(&target)?;
        props
            .SetValue(&PKEY_AppUserModel_ID, &id)
            .context("write AppUserModelID")?;
        props
            .SetValue(&PKEY_AppUserModel_ToastActivatorCLSID, &activator)
            .context("write toast activator CLSID")?;
        props.Commit().context("commit shortcut properties")?;
        file.Save(&filename, true)
            .context("save Start menu shortcut")?;
        SHChangeNotify(
            if exists {
                SHCNE_UPDATEITEM
            } else {
                SHCNE_CREATE
            },
            SHCNF_PATHW | SHCNF_FLUSH,
            Some(filename.as_ptr().cast()),
            None,
        );
    }
    Ok(())
}

pub fn show(dimensions: &str, path: Option<&Path>) -> Result<()> {
    let result = send(dimensions, path);
    if let Err(error) = &result {
        log_failure(&format!("{error:#}"));
    }
    result
}

fn send(dimensions: &str, path: Option<&Path>) -> Result<()> {
    prepare()?;
    let day = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() / 86400;
    let mut launch = COPIED.to_owned();
    let mut viewable = None;
    if let Some(file) = path {
        match remember(CURRENT_USER, file, day) {
            Ok(id) => {
                launch = format!("{SCHEME}{id}");
                viewable = Some(file);
            }
            Err(error) => eprintln!("cannot register screenshot for viewing: {error:#}"),
        }
    }
    let title = if path.is_some() {
        "Screenshot saved"
    } else {
        "Screenshot copied"
    };
    let body = crate::notification::body(dimensions, viewable);
    let xml = XmlDocument::new()?;
    let template = format!(
        r#"
        <toast activationType="protocol" launch="{launch}">
            <visual><binding template="ToastGeneric">
                <text>{title}</text><text>{body}</text>
            </binding></visual>
            <audio silent="true"/>
        </toast>"#
    );
    xml.LoadXml(&template.into())?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&APP_ID.into())?;
    if let Ok(setting) = notifier.Setting() {
        anyhow::ensure!(
            setting == NotificationSetting::Enabled,
            "Windows notification setting: {}",
            match setting {
                NotificationSetting::DisabledForApplication => "disabled for QuickShot",
                NotificationSetting::DisabledForUser => "disabled for this user",
                NotificationSetting::DisabledByGroupPolicy => "disabled by group policy",
                NotificationSetting::DisabledByManifest => "disabled by manifest",
                _ => "unknown",
            }
        );
    }
    if let Err(error) = toast.Failed(&windows::Foundation::TypedEventHandler::<
        ToastNotification,
        ToastFailedEventArgs,
    >::new(|_, args| {
        if let Some(args) = args.as_ref() {
            log_failure(&format!("toast delivery failed: {:?}", args.ErrorCode()?));
        }
        Ok(())
    })) {
        log_failure(&format!("subscribe to toast delivery errors: {error}"));
    }
    notifier.Show(&toast).context("submit toast to Windows")?;
    if viewable.is_some() {
        if let Err(error) = prune_expired(CURRENT_USER, day) {
            eprintln!("notification history cleanup: {error:#}");
        }
    }
    Ok(())
}

fn handler_executable(root: &Key, current_exe: &Path) -> PathBuf {
    root.open(INSTALL)
        .and_then(|key| key.get_hstring("InstallLocation"))
        .ok()
        .map(|dir| PathBuf::from(dir.to_os_string()).join("QuickShot.exe"))
        .filter(|exe| exe.is_absolute() && exe.is_file())
        .unwrap_or_else(|| current_exe.to_owned())
}

fn register_protocol(root: &Key, current_exe: &Path) -> Result<()> {
    let exe = handler_executable(root, current_exe);
    let scheme = root.create(PROTOCOL)?;
    scheme.set_string("", "URL:QuickShot screenshot")?;
    scheme.set_string("URL Protocol", "")?;
    let open = scheme.create(r"shell\open\command")?;
    let command = format!(r#""{}" --view "%1""#, exe.display());
    if open.get_string("").ok().as_deref() != Some(command.as_str()) {
        open.set_string("", &command)?;
    }
    Ok(())
}

fn prune_expired(root: &Key, day: u64) -> Result<()> {
    // Only maintenance needs DELETE. Recording a screenshot uses ordinary write access.
    let files = root.options().read().write().access(DELETE).open(FILES)?;
    for key in files.keys()?.collect::<Vec<_>>() {
        if key.parse::<u64>().is_ok_and(|d| day.saturating_sub(d) > 7) {
            if let Err(error) = files.remove_tree(&key) {
                eprintln!("cannot remove expired notification bucket {key}: {error}");
            }
        }
    }
    Ok(())
}

fn remember(root: &Key, file: &Path, day: u64) -> Result<String> {
    register_protocol(root, &std::env::current_exe()?)?;
    let files = root.create(FILES)?;
    let bucket = day.to_string();
    let id = unsafe { windows::Win32::System::Com::CoCreateGuid()? };
    let token = format!("{id:?}");
    files
        .create(&bucket)?
        .set_hstring(&token, &file.as_os_str().into())?;
    Ok(format!("{bucket}/{token}"))
}

pub fn view(uri: &str) -> Result<()> {
    // Clipboard-only toasts acknowledge the copy and have no file to reveal.
    if uri == COPIED {
        return Ok(());
    }
    reveal(&lookup(CURRENT_USER, uri)?)
}

fn lookup(root: &Key, uri: &str) -> Result<PathBuf> {
    let (day, token) = uri
        .strip_prefix(SCHEME)
        .and_then(|s| s.split_once('/'))
        .context("invalid QuickShot notification")?;
    anyhow::ensure!(
        day.parse::<u64>().is_ok()
            && token.len() == 36
            && token.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'),
        "invalid notification identifier"
    );
    let bucket = root.open(FILES)?.open(day)?;
    let path = bucket
        .get_hstring(token)
        .context("this screenshot notification has expired")?;
    Ok(PathBuf::from(path.to_os_string()))
}

fn reveal(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    use windows_sys::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};

    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let init = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
        anyhow::ensure!(
            init >= 0 || init == RPC_E_CHANGED_MODE,
            "initialize COM: {init:#x}"
        );
        let item = ILCreateFromPathW(wide.as_ptr());
        let result = if item.is_null() {
            Err(anyhow::anyhow!("file is no longer available"))
        } else {
            let hr = SHOpenFolderAndSelectItems(item, 0, std::ptr::null(), 0);
            ILFree(item);
            if hr < 0 {
                Err(anyhow::anyhow!("open Explorer: {hr:#x}"))
            } else {
                Ok(())
            }
        };
        if init >= 0 {
            CoUninitialize();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Exercise real registry operations under a unique test key, including cold lookup.
    struct RegistryFixture(String, Key);
    impl RegistryFixture {
        fn new() -> Self {
            let guid = unsafe { windows::Win32::System::Com::CoCreateGuid().unwrap() };
            let path = format!(r"Software\QuickShotTests-{guid:?}");
            let root = CURRENT_USER.create(&path).unwrap();
            Self(path, root)
        }
    }
    impl Drop for RegistryFixture {
        fn drop(&mut self) {
            CURRENT_USER.remove_tree(&self.0).unwrap();
        }
    }

    #[test]
    fn upgrades_a_plain_start_menu_link_with_the_toast_identity() {
        use windows::Win32::{
            Storage::EnhancedStorage::{
                PKEY_AppUserModel_ID, PKEY_AppUserModel_ToastActivatorCLSID,
            },
            System::Com::{
                CoCreateInstance, IPersistFile, StructuredStorage::PropVariantToGUID,
                CLSCTX_INPROC_SERVER, STGM_READ,
            },
            UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, ShellLink},
        };

        let _runtime = NotificationRuntime::initialize().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("QuickShot.lnk");
        let exe = std::env::current_exe().unwrap();
        let filename = HSTRING::from(path.as_path());
        unsafe {
            let plain: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            plain.SetPath(&HSTRING::from(exe.as_path())).unwrap();
            plain
                .cast::<IPersistFile>()
                .unwrap()
                .Save(&filename, true)
                .unwrap();
        }
        ensure_shortcut(&path, &exe).unwrap();
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.cast::<IPersistFile>()
                .unwrap()
                .Load(&filename, STGM_READ)
                .unwrap();
            let props: IPropertyStore = link.cast().unwrap();
            assert_eq!(
                props.GetValue(&PKEY_AppUserModel_ID).unwrap().to_string(),
                APP_ID
            );
            let clsid = props
                .GetValue(&PKEY_AppUserModel_ToastActivatorCLSID)
                .unwrap();
            assert_eq!(PropVariantToGUID(&clsid).unwrap(), ACTIVATOR);
        }
    }

    #[test]
    fn portable_uses_installed_handler_while_it_exists() {
        let fixture = RegistryFixture::new();
        let root = &fixture.1;
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("QuickShot.exe");
        let portable = dir.path().join("portable copy").join("QuickShot.exe");
        root.create(INSTALL)
            .unwrap()
            .set_hstring("InstallLocation", &dir.path().as_os_str().into())
            .unwrap();
        for expected in [&portable, &installed, &portable] {
            if expected == &installed {
                std::fs::write(&installed, []).unwrap();
            } else if installed.exists() {
                std::fs::remove_file(&installed).unwrap();
            }
            register_protocol(root, &portable).unwrap();
            let command = root
                .open(PROTOCOL)
                .unwrap()
                .open(r"shell\open\command")
                .unwrap();
            assert_eq!(
                command.get_string("").unwrap(),
                format!(r#""{}" --view "%1""#, expected.display())
            );
        }
    }

    #[test]
    fn cleanup_preserves_each_live_notification_mapping() {
        let fixture = RegistryFixture::new();
        let root = &fixture.1;
        let file = Path::new(r"C:\截图 1.png");
        let other = Path::new(r"C:\截图 2.png");
        let expired = remember(root, file, 10).unwrap();
        let first = remember(root, file, 11).unwrap();
        let second = remember(root, other, 18).unwrap();
        prune_expired(root, 18).unwrap();
        let reopened = CURRENT_USER.open(&fixture.0).unwrap();
        let resolve = |id| lookup(&reopened, &format!("{SCHEME}{id}"));
        assert!(resolve(expired).is_err());
        assert_eq!(resolve(first).unwrap(), file);
        assert_eq!(resolve(second).unwrap(), other);
    }
}
