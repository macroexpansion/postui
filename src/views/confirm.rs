//! Confirmation modal as a View. Carries an action closure that runs on `y`.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect};
use std::sync::Arc;

use crate::{
    ui::{confirm::Confirm, theme::Theme},
    views::{AppEvent, Ctx, Outcome, View, ViewId, ViewPayload},
};

pub struct ConfirmView {
    id: ViewId,
    confirm: Confirm,
    /// Action returns a future that resolves to a textual op result.
    action: Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<String, crate::error::DbError>> + Send + Sync>,
}

impl ConfirmView {
    pub fn new<F, Fut>(title: impl Into<String>, body: impl Into<String>, sql: impl Into<String>, action: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<String, crate::error::DbError>> + Send + 'static,
    {
        use futures::FutureExt;
        let action: Arc<dyn Fn() -> futures::future::BoxFuture<'static, Result<String, crate::error::DbError>> + Send + Sync>
            = Arc::new(move || action().boxed());
        Self {
            id: ViewId::next(),
            confirm: Confirm { title: title.into(), body: body.into(), sql: sql.into() },
            action,
        }
    }
}

impl View for ConfirmView {
    fn id(&self) -> ViewId { self.id }
    fn title(&self) -> &str { "confirm" }

    fn render(&mut self, f: &mut Frame, area: Rect, theme: &Theme) {
        self.confirm.render(f, area, theme);
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Ctx) -> Outcome {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let view_id = self.id;
                let tx = ctx.event_tx.clone();
                let action = self.action.clone();
                tokio::spawn(async move {
                    let res = (action)().await;
                    let _ = tx.send(AppEvent::ViewData {
                        view_id,
                        payload: ViewPayload::OpResult(res),
                    }).await;
                });
                Outcome::Pop
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Outcome::Pop,
            _ => Outcome::Consumed,
        }
    }
}
