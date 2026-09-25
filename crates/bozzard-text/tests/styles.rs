use bozzard_text::{Font, bounds_with_font};
use std::collections::BTreeMap;

fn variable() -> anyhow::Result<Font> {
    Font::parse(include_bytes!("../assets/Roboto.ttf").to_vec())
}
fn width(font: &Font, text: &str) -> anyhow::Result<f32> {
    let b = bounds_with_font(text, 1., None, false, 0, Some(font))?.unwrap();
    Ok(b[1][0] - b[0][0])
}
#[test]
fn axes_change_real_metrics_and_keep_stable_identity_through_repeated_extraction()
-> anyhow::Result<()> {
    let font = variable()?;
    assert!(font.axes().iter().any(|a| a.tag == "wght"));
    let settings = BTreeMap::from([("wdth".into(), 75.), ("wght".into(), 900.)]);
    let narrow = font.styled(&settings, &[], false)?;
    let normal = font.styled(&BTreeMap::from([("wdth".into(), 100.)]), &[], false)?;
    assert_eq!(
        normal.key(),
        font.key(),
        "explicit default coordinates should reuse the default style"
    );
    assert!(width(&narrow, "Wide lettering")? < width(&normal, "Wide lettering")?);
    let heavy = font.styled(&BTreeMap::from([("wght".into(), 900.)]), &[], false)?;
    assert_ne!(
        width(&heavy, "Wide lettering")?,
        width(&normal, "Wide lettering")?
    );
    for _ in 0..100 {
        let again = font.styled(&settings, &[], false)?;
        assert_eq!(again.key(), narrow.key());
        assert_eq!(
            width(&again, "Wide lettering")?,
            width(&narrow, "Wide lettering")?
        );
    }
    for bad in [0., 101., f32::NAN, f32::INFINITY] {
        assert!(
            font.styled(&BTreeMap::from([("wdth".into(), bad)]), &[], false)
                .is_err()
        );
    }
    assert!(
        font.styled(&BTreeMap::from([("XXXX".into(), 1.)]), &[], false)
            .is_err()
    );
    Ok(())
}

#[test]
fn missing_glyphs_use_custom_or_bundled_fallbacks_and_reloads_change_identity() -> anyhow::Result<()>
{
    use skrifa::MetadataProvider;
    let primary_bytes = include_bytes!("../assets/Roboto.ttf");
    let fallback_bytes = include_bytes!("../../bozzard-assets/tests/fonts/test.ttf");
    let primary = variable()?;
    let fallback = Font::parse(fallback_bytes.to_vec())?;
    let primary_map = skrifa::FontRef::new(primary_bytes)?.charmap();
    let fallback_map = skrifa::FontRef::new(fallback_bytes)?.charmap();
    let character = (0x2000..0xffff)
        .filter_map(char::from_u32)
        .find(|c| primary_map.map(*c).is_none() && fallback_map.map(*c).is_some())
        .expect("fixture has extra glyph coverage");
    let text = character.to_string().repeat(5);
    let custom = primary.styled(&Default::default(), std::slice::from_ref(&fallback), false)?;
    assert_eq!(width(&custom, &text)?, width(&fallback, &text)?);
    assert_ne!(width(&primary, &text)?, width(&custom, &text)?);
    let bundled = primary.styled(&Default::default(), &[], true)?;
    assert_ne!(width(&primary, "😀😀")?, width(&bundled, "😀😀")?);
    let definitions = epaint::text::FontDefinitions::default();
    let emoji_data = &definitions.font_data["NotoEmoji-Regular"].font;
    let emoji = Font::parse(emoji_data.to_vec())?;
    let emoji_map = skrifa::FontRef::new(emoji_data)?.charmap();
    let shared = (0x2000..0xffff)
        .filter_map(char::from_u32)
        .find(|c| {
            primary_map.map(*c).is_none()
                && fallback_map.map(*c).is_some()
                && emoji_map.map(*c).is_some()
        })
        .expect("fallbacks share extra coverage");
    let shared = shared.to_string().repeat(3);
    let first = primary.styled(
        &Default::default(),
        &[fallback.clone(), emoji.clone()],
        false,
    )?;
    let reversed = primary.styled(
        &Default::default(),
        &[emoji.clone(), fallback.clone()],
        false,
    )?;
    assert_eq!(width(&first, &shared)?, width(&fallback, &shared)?);
    assert_eq!(width(&reversed, &shared)?, width(&emoji, &shared)?);
    assert_ne!(width(&first, &shared)?, width(&reversed, &shared)?);
    assert!(
        primary
            .styled(&Default::default(), std::slice::from_ref(&primary), false)
            .is_err()
    );
    assert!(
        primary
            .styled(
                &Default::default(),
                &[fallback.clone(), fallback.clone()],
                false
            )
            .is_err()
    );
    let replacement = Font::parse(fallback_bytes.to_vec())?;
    assert_ne!(
        custom.key(),
        primary
            .styled(&Default::default(), &[replacement], false)?
            .key()
    );
    Ok(())
}
