use std::error::Error;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetPropertyReply, Window};
use x11rb::rust_connection::RustConnection;

// https://www.reddit.com/r/rust/comments/f7yrle/get_information_about_current_window_xorg/
// The return can be a empty result or boxed error.
pub fn get_active_window() -> Result<String, Box<dyn Error>> {
    // Set up our state
    let (conn, screen) = x11rb::connect(None).expect("Failed to connect");
    let root = conn.setup().roots[screen].root;
    let net_active_window = get_or_intern_atom(&conn, b"_NET_ACTIVE_WINDOW");
    let net_wm_name = get_or_intern_atom(&conn, b"_NET_WM_NAME");
    let utf8_string = get_or_intern_atom(&conn, b"UTF8_STRING");

    let focus = match find_active_window(&conn, root, net_active_window) {
        None => {
            println!("No active window selected");
            return Ok("".to_string());
        }
        Some(x) => x,
    };

    // Collect the replies to the atoms
    let (net_wm_name, utf8_string) = (net_wm_name, utf8_string);
    let (wm_class, string): (Atom, Atom) = (AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into());

    // Get the property from the window that we need
    let name = conn.get_property(false, focus, net_wm_name, utf8_string, 0, u32::MAX)?;
    let class = conn.get_property(false, focus, wm_class, string, 0, u32::MAX)?;
    let (name, class) = (name.reply()?, class.reply()?);

    let (_window_instance, class) = parse_wm_class(&class);
    let _window_name = parse_string_property(&name);
    let window_class = class;
    Ok(window_class)
}

fn get_or_intern_atom(conn: &RustConnection, name: &[u8]) -> Atom {
    let result = conn
        .intern_atom(false, name)
        .expect("Failed to intern atom")
        .reply()
        .expect("Failed receive interned atom");

    result.atom
}

fn parse_string_property(property: &GetPropertyReply) -> String {
    std::str::from_utf8(&property.value)
        .unwrap_or("Invalid utf8")
        .to_string()
}

fn find_active_window(
    conn: &impl Connection,
    root: Window,
    net_active_window: Atom,
) -> Option<Window> {
    let window: Atom = AtomEnum::WINDOW.into();
    let active_window = conn
        .get_property(false, root, net_active_window, window, 0, 1)
        .expect("Failed to get X11 property")
        .reply()
        .expect("Failed to receive X11 property reply");

    if active_window.format == 32 && active_window.length == 1 {
        active_window
            .value32()
            .expect("Invalid message. Expected value with format = 32")
            .next()
    } else {
        // Query the input focus
        Some(
            conn.get_input_focus()
                .expect("Failed to get input focus")
                .reply()
                .expect("Failed to receive X11 input focus")
                .focus,
        )
    }
}

fn parse_wm_class(property: &GetPropertyReply) -> (String, String) {
    if property.format != 8 {
        return (
            "Malformed property: wrong format".to_string(),
            "Malformed property: wrong format".to_string(),
        );
    }
    let value = &property.value;
    // The property should contain two null-terminated strings. Find them.
    if let Some(middle) = value.iter().position(|&b| b == 0) {
        let (instance, class) = value.split_at(middle);
        // Skip the null byte at the beginning
        let mut class = &class[1..];
        // Remove the last null byte from the class, if it is there.
        if class.last() == Some(&0) {
            class = &class[..class.len() - 1];
        }
        let instance = std::str::from_utf8(instance);
        let class = std::str::from_utf8(class);
        (
            instance.unwrap_or("Invalid utf8").to_string(),
            class.unwrap_or("Invalid utf8").to_string(),
        )
    } else {
        (
            "Missing null byte".to_string(),
            "Missing null byte".to_string(),
        )
    }
}
