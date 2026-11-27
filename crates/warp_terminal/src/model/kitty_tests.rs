use super::*;

fn action(chunks: &[&[u8]]) -> Result<KittyAction, KittyError> {
    let mut chunks = chunks.iter().map(|bytes| parse_kitty_chunk(bytes));
    let mut pending = PendingKittyMessage::new(chunks.next().unwrap());
    for chunk in chunks {
        pending.append(chunk);
    }
    KittyAction::try_from(KittyMessage::try_from(pending)?)
}

#[test]
fn streams_raw_pixels_across_chunks() {
    let command = action(&[b"a=T,f=24,i=2,s=2,v=1,m=1;/wAA", b"m=0;AP8A"]).unwrap();
    let KittyAction::StoreAndDisplay(image) = command else {
        panic!("expected a displayed image")
    };
    assert_eq!(image.image_id, 2);
    assert_eq!(image.image.metadata.image_size, Vector2F::new(2., 1.));
    assert!(image.image.data.ends_with(&[255, 0, 0, 0, 255, 0]));
}

#[test]
fn query_accepts_a_direct_rgb_pixel() {
    assert!(matches!(
        action(&[b"a=q,f=24,i=31,s=1,v=1;AAAA"]),
        Ok(KittyAction::QuerySupport(_))
    ));
}

#[test]
fn queries_validate_encoding_and_pixels_before_replying_ok() {
    for request in [
        b"a=q,f=24,s=1,v=1;AA==".as_slice(),
        b"a=q,f=71,s=1,v=1;AAAAAA==",
        b"a=q,t=x,s=1,v=1;AAAAAA==",
        b"a=q,o=x,s=1,v=1;AAAAAA==",
    ] {
        assert!(action(&[request]).is_err());
    }
    use image::ImageEncoder;
    let mut png = vec![];
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &[255, 0, 0, 0, 255, 0],
            1,
            2,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let query = |bytes: &[u8]| {
        format!(
            "a=q,i=42,f=100;{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    };
    let KittyAction::QuerySupport(query_result) = action(&[query(&png).as_bytes()]).unwrap() else {
        panic!()
    };
    assert!(
        query_result
            .image
            .data
            .ends_with(&[255, 0, 0, 255, 0, 255, 0, 255])
    );
    assert!(action(&[query(&png[..32]).as_bytes()]).is_err());
}

#[test]
fn separate_omp_row_placements_reference_different_bands_of_one_upload() {
    use crate::event_listener::ChannelEventListener;
    use crate::model::ansi::Handler;
    use crate::model::blockgrid::BlockGrid;
    use crate::model::grid::grid_handler::PerformResetGridChecks;
    use crate::model::image_map::StoredImageMetadata;
    use crate::model::secrets::ObfuscateSecrets;
    use warp_core::features::FeatureFlag;

    let _enabled = FeatureFlag::KittyImages.override_enabled(true);
    let mut grid = BlockGrid::new(
        crate::SizeInfo::new_without_font_metrics(10, 10),
        100,
        ChannelEventListener::new_for_test(),
        ObfuscateSecrets::No,
        PerformResetGridChecks::default(),
    );
    let upload = action(&[b"a=t,i=21,f=24,s=1,v=2;/wAAAP8A"]).unwrap();
    let KittyAction::StoreOnly(stored) = &upload else {
        panic!()
    };
    let size = stored.image.metadata.image_size;
    let mut metadata = std::collections::HashMap::from([(
        21,
        StoredImageMetadata::Kitty(stored.image.metadata.clone()),
    )]);
    grid.handle_completed_kitty_action(upload, &mut metadata)
        .unwrap()
        .unwrap();
    for (id, y) in [(11, 0), (12, 1)] {
        let request = format!("a=p,C=1,i=21,p={id},x=0,y={y},w=1,h=1,c=4,r=1");
        grid.handle_completed_kitty_action(action(&[request.as_bytes()]).unwrap(), &mut metadata)
            .unwrap()
            .unwrap();
    }
    for (id, y) in [(11, 0.), (12, 1.)] {
        let rect = grid
            .grid_handler()
            .get_image_placement_data(21, id)
            .unwrap()
            .source
            .resolve(size)
            .unwrap();
        assert_eq!(rect.origin(), Vector2F::new(0., y));
        assert_eq!(rect.size(), Vector2F::new(1., 1.));
    }
    assert_eq!(metadata.len(), 1);
    assert!(
        KittySourceRect {
            x: u32::MAX,
            ..Default::default()
        }
        .resolve(size)
        .is_none()
    );
}

#[test]
fn invalid_chunks_do_not_display_a_partial_image() {
    assert!(action(&[b"a=T,f=24,s=1,v=1,m=1;!", b"m=0;AAAA"]).is_err());
    let mut too_large = b"a=T,f=24,s=1,v=1;".to_vec();
    too_large.resize(MAX_KITTY_APC_BYTES + 1, b'A');
    assert!(action(&[&too_large]).is_err());
}

#[test]
fn a_delete_interrupts_an_unfinished_transmission_and_keeps_its_own_id() {
    let request = action(&[b"a=T,f=24,s=2,v=1,i=2,m=1;/wAA", b"a=d,d=i,i=8,p=3"]).unwrap();
    let KittyAction::Delete {
        deletion_type: DeletionType::DeleteById(target),
        ..
    } = request
    else {
        panic!()
    };
    assert_eq!((target.image_id, target.placement_id), (8, Some(3)));
}

#[test]
fn refuses_excessive_dimensions_before_allocating_a_bitmap() {
    assert!(action(&[b"a=T,f=32,s=4294967295,v=4294967295;AAAA"]).is_err());
}

#[cfg(windows)]
#[test]
fn file_and_shared_memory_transfers_are_refused() {
    for request in [
        b"a=q,f=24,s=1,v=1,t=f;YQ==",
        b"a=q,f=24,s=1,v=1,t=t;YQ==",
        b"a=q,f=24,s=1,v=1,t=s;YQ==",
    ] {
        assert!(action(&[request]).is_err());
    }
}

#[test]
fn targeted_deletion_keeps_its_target() {
    let command = action(&[b"a=d,d=i,i=4,p=2"]).unwrap();
    let KittyAction::Delete {
        delete_placements_only,
        deletion_type: DeletionType::DeleteById(target),
    } = command
    else {
        panic!("expected targeted deletion")
    };
    assert!(delete_placements_only);
    assert_eq!(target.image_id, 4);
    assert_eq!(target.placement_id, Some(2));
    assert!(action(&[b"a=d,d=x"]).is_err());
}

#[test]
fn delete_visible_kitty_placements_preserves_scrollback_virtual_and_iterm_images() {
    use crate::model::grid::grid_handler::{AbsolutePoint, AbsoluteRectangle};
    use crate::model::image_map::{ImageMap, ImagePlacementData, ImageType, VirtualPlacement};
    let mut images = ImageMap::default();
    for (id, row, protocol) in [
        (1, 1, ImageType::Kitty),
        (2, 2, ImageType::Kitty),
        (3, 2, ImageType::ITerm),
    ] {
        images.add_image_placement_data(
            id,
            1,
            ImagePlacementData {
                source: Default::default(),
                z_index: 0,
                height_cells: 1,
                image_size: Vector2F::new(2., 1.),
            },
        );
        images.place(id, 1, AbsolutePoint { row, col: 0 }, protocol, 0);
    }
    images.add_virtual_placement(
        2,
        VirtualPlacement {
            source: Default::default(),
            placement_id: 2,
            cols: 1,
            rows: 1,
        },
    );
    let removed = images.evict_kitty_in_rectangle(AbsoluteRectangle {
        start_row: 2,
        end_row: 2,
    });
    assert_eq!(removed, [2].into_iter().collect());
    assert!(images.get_image_placement_data(1, 1).is_some());
    assert!(images.get_image_placement_data(2, 1).is_none());
    assert!(images.get_image_placement_data(3, 1).is_some());
    assert!(images.virtual_placement(2).is_some());
}

#[test]
fn replacing_a_placement_moves_it_from_its_previous_cells() {
    use crate::model::grid::grid_handler::{AbsolutePoint, AbsoluteRectangle};
    use crate::model::image_map::{ImageMap, ImagePlacementData, ImageType};
    let mut images = ImageMap::default();
    images.add_image_placement_data(
        9,
        1,
        ImagePlacementData {
            source: Default::default(),
            z_index: 0,
            height_cells: 1,
            image_size: Vector2F::new(1., 1.),
        },
    );
    for row in [1, 3] {
        images.place(9, 1, AbsolutePoint { row, col: 0 }, ImageType::Kitty, 0);
    }
    let placements = images.get_image_ids_by_rectangle(AbsoluteRectangle {
        start_row: 0,
        end_row: 4,
    });
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0].top_left.row, 3);
}
