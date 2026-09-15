//! Platform-neutral AccessKit semantics from the same layout used for drawing and hit testing.
use accesskit::{Action, ActionData, ActionRequest, Live, Node, Rect, Role, Toggled};
use bozzard_scene::middleware::ui::{Element, Input, WidgetKind};
pub fn node(element: &Element, offset: [f32; 2], scale: f32) -> Node {
    let role = match element.widget.kind {
        WidgetKind::Panel => Role::GenericContainer,
        WidgetKind::Label => Role::Label,
        WidgetKind::Image => Role::Image,
        WidgetKind::Button => Role::Button,
        WidgetKind::Toggle => Role::CheckBox,
        WidgetKind::Slider => Role::Slider,
    };
    let mut node = Node::new(role);
    let rect = element.rect.intersect(element.clip);
    node.set_bounds(Rect {
        x0: ((rect.min[0] + offset[0]) * scale) as f64,
        y0: ((rect.min[1] + offset[1]) * scale) as f64,
        x1: ((rect.min[0] + rect.size[0] + offset[0]) * scale) as f64,
        y1: ((rect.min[1] + rect.size[1] + offset[1]) * scale) as f64,
    });
    node.set_label(element.label.clone());
    if element.widget.scrollable {
        node.add_action(Action::ScrollUp);
        node.add_action(Action::ScrollDown);
        node.set_scroll_y(element.scroll as f64);
        node.set_scroll_y_min(0.);
        node.set_scroll_y_max(element.scroll_max as f64);
    }
    if !element.description.is_empty() {
        node.set_description(element.description.clone());
    }
    if !element.enabled {
        node.set_disabled();
    }
    if element.enabled && element.widget.kind.interactive() {
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
    }
    match element.widget.kind {
        WidgetKind::Toggle => node.set_toggled(
            if element.value > (element.widget.min + element.widget.max) * 0.5 {
                Toggled::True
            } else {
                Toggled::False
            },
        ),
        WidgetKind::Slider => {
            node.set_numeric_value(element.value as f64);
            node.set_min_numeric_value(element.widget.min as f64);
            node.set_max_numeric_value(element.widget.max as f64);
            node.set_numeric_value_step(element.widget.step as f64);
            if element.enabled {
                node.add_action(Action::Increment);
                node.add_action(Action::Decrement);
                node.add_action(Action::SetValue);
            }
        }
        WidgetKind::Label if !element.widget.binding.is_empty() => node.set_live(Live::Polite),
        _ => {}
    }
    node
}
pub fn action(element: &Element, request: &ActionRequest) -> Option<Input> {
    if element.enabled
        && element.widget.scrollable
        && matches!(request.action, Action::ScrollUp | Action::ScrollDown)
    {
        return Some(Input::ScrollObject {
            owner: element.owner.clone(),
            delta: element.rect.size[1]
                * 0.8
                * if request.action == Action::ScrollUp {
                    -1.
                } else {
                    1.
                },
        });
    }
    if !element.enabled || !element.widget.kind.interactive() {
        return None;
    }
    Some(match request.action {
        Action::Focus => Input::Focus(element.owner.clone()),
        Action::Click => Input::ActivateObject(element.owner.clone()),
        Action::Increment | Action::Decrement if element.widget.kind == WidgetKind::Slider => {
            Input::SetValue {
                owner: element.owner.clone(),
                value: element.value
                    + element.widget.step
                        * if request.action == Action::Increment {
                            1.
                        } else {
                            -1.
                        },
            }
        }
        Action::SetValue if element.widget.kind == WidgetKind::Slider => {
            let Some(ActionData::NumericValue(value)) = &request.data else {
                return None;
            };
            if !value.is_finite() || value.abs() > f32::MAX as f64 {
                return None;
            }
            Input::SetValue {
                owner: element.owner.clone(),
                value: *value as f32,
            }
        }
        _ => return None,
    })
}
