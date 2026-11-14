//! M2-A：全部 28 个上游 profile 载入 + 形状健全性。
use spektra_core::profile::load_profile;

const STOCKS: &[&str] = &[
    "fujifilm_c200", "fujifilm_crystal_archive_typeii", "fujifilm_pro_400h",
    "fujifilm_provia_100f", "fujifilm_velvia_100", "fujifilm_xtra_400",
    "kodak_2383", "kodak_2393", "kodak_ektachrome_100", "kodak_ektacolor_edge",
    "kodak_ektar_100", "kodak_endura_premier", "kodak_gold_200", "kodak_kodachrome_64",
    "kodak_portra_160", "kodak_portra_400", "kodak_portra_800", "kodak_portra_800_push1",
    "kodak_portra_800_push2", "kodak_portra_endura", "kodak_supra_endura",
    "kodak_ultra_endura", "kodak_ultramax_400", "kodak_verita_200d",
    "kodak_vision3_200t", "kodak_vision3_250d", "kodak_vision3_500t", "kodak_vision3_50d",
];

#[test]
fn all_28_profiles_load() {
    assert_eq!(STOCKS.len(), 28);
    let mut films = 0;
    let mut papers = 0;
    let mut positives = 0;
    for stock in STOCKS {
        let p = load_profile(stock).unwrap_or_else(|e| panic!("{stock} 加载失败: {e}"));
        assert_eq!(p.data.wavelengths.len(), 81, "{stock}");
        assert_eq!(p.data.log_exposure.len(), 256, "{stock}");
        assert_eq!(p.data.density_curves.len(), 256, "{stock}");
        assert_eq!(p.data.channel_density.len(), 81, "{stock}");
        match p.info.support.as_str() {
            "film" => films += 1,
            "paper" => papers += 1,
            other => panic!("{stock}: 未知 support {other}"),
        }
        if p.info.film_type == "positive" {
            positives += 1;
        }
    }
    println!("films={films} papers={papers} positives={positives}");
    assert_eq!(films, 22);
    assert_eq!(papers, 6);
    assert_eq!(positives, 4);
}

#[test]
fn films_have_erf4_window_papers_have_none() {
    for stock in STOCKS {
        let p = load_profile(stock).unwrap();
        let n = p.data.window_params.len();
        match p.info.support.as_str() {
            // 彩色胶片 4 参数 erf4 窗；cinema 正片拷贝（2383/2393）与相纸无窗
            "film" => assert!(n == 4 || n == 0, "{stock} 窗口参数数异常: {n}"),
            "paper" => assert_eq!(n, 0, "{stock} 相纸应无窗口参数"),
            _ => {}
        }
    }
}
