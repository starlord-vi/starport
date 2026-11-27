// SPDX-License-Identifier: GPL-3.0-or-later
//! A descriptor used only for its NUM_RECORDS validity test, not a data read.
use super::host::{Dispatch, Host, Root, Stride, View};
use super::*;
use crate::native::{ComputeIo, ResourceType, RootParam, StageIo, UsedResource};

#[test]
#[ignore = "requires external STARPORT_DESCRIPTOR_COUNT_FIXTURE and NVIDIA hardware"]
fn captured_depth_reduce_shader_validates_and_creates_pipeline() {
    let path =
        std::path::PathBuf::from(std::env::var("STARPORT_DESCRIPTOR_COUNT_FIXTURE").unwrap());
    let path = path.canonicalize().unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(
        !path.starts_with(repo),
        "proprietary fixture must remain outside Git"
    );
    let bytes = std::fs::read(path).unwrap();
    let mut shader = translate(&bytes).expect("captured depth reduction must translate");
    assert_eq!(shader.stage, HwStage::Cs);
    assert!(
        shader
            .user_data
            .iter()
            .any(|u| u.root == RootType::Other(USER_VIEW_RECORDS))
    );
    validator::Validator::load(validator::Validator::default_path().as_deref())
        .expect("DXIL validator required")
        .validate(&mut shader.dxil)
        .unwrap();
    let host = Host::new(false).expect("hardware device required");
    host.compute_pipeline(&shader)
        .expect("captured shader must create a hardware pipeline");
}

#[test]
fn buffer_descriptor_record_count_branches_execute_on_nvidia() {
    let _one = super::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // s4/s6 receive the two tables, then s[8:11] is used only as numbers.
    let words: [u32; 18] = [
        0xBE84_0300,
        0xBE85_0380, // s[4:5] = table 0
        0xBE86_0301,
        0xBE87_0380, // s[6:7] = table 1
        0xF408_0202,
        0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], 0
        0xBF06_800A, // s_cmp_eq_u32 s10, 0
        0xBF85_0002, // zero -> literal 0x55
        0x7E04_020A, // v_mov_b32 v2, s10
        0xBF82_0002, // skip zero case
        0x7E04_02FF,
        0x55, // v_mov_b32 v2, 0x55
        0xF408_0303,
        0xFA00_0000, // s_load_dwordx4 s[12:15], s[6:7], 0
        0xBF8C_C07F, // s_waitcnt
        0xE070_0000,
        0x8003_0200, // buffer_store_dword v2, off, s[12:15], 0
        0xBF81_0000, // s_endpgm
    ];
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: false,
        vgprs: 16,
        layout: (0..2)
            .map(|slot| RootParam {
                root: RootType::Table,
                table_offset_dwords: 0,
                sgprs: 1,
                base_register: 0,
                user_data: slot,
                slot,
            })
            .collect(),
        used: vec![
            UsedResource {
                kind: ResourceType::Srv,
                root: RootType::Table,
                table_offset_dwords: 0,
                lower_bound: 0,
                stride: 16,
                dwords: 4,
                user_data: 0,
                slot: 0,
            },
            UsedResource {
                kind: ResourceType::Uav,
                root: RootType::Table,
                table_offset_dwords: 0,
                lower_bound: 0,
                stride: 0,
                dwords: 4,
                user_data: 1,
                slot: 1,
            },
        ],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 2 << 1,
            threads: [1, 1, 1],
        }),
        code: words.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let p = ir::build(&native.code, false, native.vgprs).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
    let raised = io::raise(&p, &native).unwrap();
    let out = lower::lower(&lower::Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &raised,
        res: &res,
        inputs: vec![],
        outputs: vec![],
        prims: vec![],
        mesh_stride: 0,
    })
    .unwrap();
    let mut shader = HostShader {
        stage: HwStage::Cs,
        dxil: out.dxil,
        user_data: out.user_data,
        samplers: out.samplers,
        static_samplers: vec![],
        root_cbvs: out.root_cbvs,
        root_srvs: out.root_srvs,
        root_uavs: out.root_uavs,
        root_indexed_views: res.root_indexed_views,
        half_views: out.half_views,
    };
    assert!(
        shader
            .user_data
            .iter()
            .any(|u| u.root == RootType::Other(USER_VIEW_RECORDS))
    );
    validator::Validator::load(validator::Validator::default_path().as_deref())
        .expect("DXIL validator required")
        .validate(&mut shader.dxil)
        .unwrap();
    let host = Host::new(false).expect("NVIDIA hardware device required");
    let (root, pso) = host
        .compute_pipeline(&shader)
        .expect("hardware pipeline required");
    for records in [0, 1, 17, u32::MAX] {
        let mut data = vec![0; shader.user_data_dwords() as usize];
        for u in &shader.user_data {
            data[u.index as usize] = match (u.root, u.slot) {
                (RootType::Other(USER_VIEW_RECORDS), 0) => records,
                (RootType::Other(USER_VIEW_ADDRESS), 0) => 0x1000,
                (RootType::Other(x), 0) if x == USER_VIEW_ADDRESS + 1 => 0,
                (RootType::Table, 0) => 32,
                (RootType::Table, 1) => 0,
                other => panic!("unexpected user data {other:?}"),
            };
        }
        let result = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(data))],
                    views: &[
                        View::RawUav(1),
                        View::Buffer {
                            data: &[0; 4],
                            stride: Stride::Structured(16),
                        },
                    ],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap();
        assert_eq!(result[0][0], if records == 0 { 0x55 } else { records });
    }
}
