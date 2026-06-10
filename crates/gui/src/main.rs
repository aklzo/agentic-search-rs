mod app;
mod history;
mod runner;

use gpui::*;
use gpui_component::Root;
use gpui_component_assets::Assets;

use app::ResearchApp;

fn main() {
    let application = Application::new().with_assets(Assets);

    application.run(move |cx| {
        gpui_component::init(cx);
        cx.activate(true);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1100.), px(760.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Agentic Search".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| ResearchApp::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
