use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

use crate::common::*;
use crate::platform::common::*;

use anyhow::*;
use tracing::*;

#[cfg(feature = "x11")]
pub struct X11Ctx {
    conn: RustConnection,
    screen_num: usize,
}

#[cfg(feature = "x11")]
impl X11Ctx {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)?;
        Ok(Self { conn, screen_num })
    }
}

/// This function upload the time for the entry in the vector only if we change window to reduce the
/// overload by not going through the vector every second.
pub async fn handle_active_window(x11: &X11Ctx, procs_data: &mut ProcessTracker) -> Result<()> {
    let (w_name, w_class) = get_focused_window(x11)?;
    let now = uptime()?;

    // First run: no last window recorded
    if procs_data.last_wname.is_empty() {
        debug!("First run, recording initial window: '{w_name}'");
        procs_data.procs.push(ProcessInfo {
            w_name: w_name.clone(),
            w_time: 0,
            w_class: w_class.clone(),
        });
        procs_data.last_wname = w_name;
        procs_data.last_wclass = w_class;
        procs_data.time = now;
        return Ok(());
    }

    // Window changed
    if procs_data.last_wname != w_name {
        let elapsed = now - procs_data.time;
        debug!(
            "Focus changed, Window: '{}' was active for: {}s",
            procs_data.last_wclass, elapsed
        );
        debug!("Starting counting time for the new window: '{w_name}'");

        // Update time for the *previous* window
        if let Some(prev) = procs_data
            .procs
            .iter_mut()
            .find(|p| p.w_name == procs_data.last_wname)
        {
            prev.w_time += elapsed;
        } else {
            procs_data.procs.push(ProcessInfo {
                w_name: procs_data.last_wname.clone(),
                w_time: elapsed,
                w_class: procs_data.last_wclass.clone(),
            });
        }

        // Record the new window
        if procs_data.procs.iter().all(|p| p.w_name != w_name) {
            procs_data.procs.push(ProcessInfo {
                w_name: w_name.clone(),
                w_time: 0,
                w_class: w_class.clone(),
            });
        }

        procs_data.last_wname = w_name;
        procs_data.last_wclass = w_class;
        procs_data.time = now;
    }

    Ok(())
}

// https://www.reddit.com/r/rust/comments/f7yrle/get_information_about_current_w_xorg/
// Returns name and class of the focused window in that order.
/// Find focused window in x11 environment.
pub fn get_focused_window(x11: &X11Ctx) -> Result<(String, String)> {
    let root = x11.conn.setup().roots[x11.screen_num].root;
    let net_active_window = get_or_intern_atom(&x11.conn, b"_NET_ACTIVE_WINDOW");
    let net_wm_name = get_or_intern_atom(&x11.conn, b"_NET_WM_NAME");
    let utf8_string = get_or_intern_atom(&x11.conn, b"UTF8_STRING");

    let focus = find_active_window(&x11.conn, root, net_active_window)?;

    let (wm_class, string): (Atom, Atom) = (AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());

    // Get the property from the window we need
    let name = x11
        .conn
        .get_property(false, focus, net_wm_name, utf8_string, 0, u32::MAX)?;
    let class = x11
        .conn
        .get_property(false, focus, wm_class, string, 0, u32::MAX)?;
    let (name, class) = (name.reply()?, class.reply()?);

    let class = parse_wm_class(&class)?;
    let name = parse_string_property(&name)?.to_string();
    let class = class.to_string();

    Ok((name, class))
}

fn get_or_intern_atom(conn: &RustConnection, name: &[u8]) -> Atom {
    let result = conn
        .intern_atom(false, name)
        .expect("Failed to intern atom")
        .reply()
        .expect("Failed receive interned atom");

    result.atom
}

fn find_active_window(
    conn: &impl Connection,
    root: Window,
    net_active_window: Atom,
) -> Result<Window> {
    let window: Atom = AtomEnum::WINDOW.into();
    let active_window = conn
        .get_property(false, root, net_active_window, window, 0, 1)?
        .reply()?;

    if active_window.format == 32 && active_window.length == 1 {
        active_window
            .value32()
            .with_context(|| "Invalid message. Expected value with format = 32")?
            .next()
            .ok_or(anyhow!("Failed to get next value"))
    } else {
        // Query the input focus
        Ok(conn
            .get_input_focus()
            .with_context(|| "Failed to get input focus")?
            .reply()?
            .focus)
    }
}

fn parse_string_property(property: &GetPropertyReply) -> Result<&str> {
    std::str::from_utf8(&property.value)
        .map_err(|err| anyhow!("Failed to parse string from utf8: {err:?}"))
}

fn parse_wm_class(property: &GetPropertyReply) -> Result<&str> {
    if property.format != 8 {
        anyhow::bail!("Failed to parse instance and class strings for window");
    }
    let value = &property.value;
    // The property should contain two null-terminated strings. Find them.
    if let Some(middle) = value.iter().position(|&b| b == 0) {
        let (_, class) = value.split_at(middle);
        // Skip the null byte at the beginning
        let mut class = &class[1..];
        // Remove the last null byte from the class, if it is there.
        if class.last() == Some(&0) {
            class = &class[..class.len() - 1];
        }
        std::str::from_utf8(class)
            .map_err(|err| anyhow!("Window class is not a valid utf8 string: {err:?}"))
    } else {
        anyhow::bail!("Failed to parse instance and class strings for window");
    }
}
