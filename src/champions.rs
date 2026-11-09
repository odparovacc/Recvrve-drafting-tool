//! Champion table. A champion's node value is `FIRST_CHAMPION_VALUE + index`, where the
//! index comes from ordering by release date and then alphabetically within a release day.
//!
//! New champions must be appended at the end so existing values never change.

pub struct ChampionInfo {
    /// Name shown in the GUI.
    pub name: &'static str,
    /// Icon file stem in `champion_icons/` (Riot's internal id, e.g. `MonkeyKing` for Wukong).
    pub key: &'static str,
    /// Release date, `YYYY-MM-DD`. Only used to document and test the ordering.
    pub released: &'static str,
}

/// Node value of the first champion (Alistar).
pub const FIRST_CHAMPION_VALUE: u8 = 16;

const fn c(name: &'static str, key: &'static str, released: &'static str) -> ChampionInfo {
    ChampionInfo {
        name,
        key,
        released,
    }
}

pub const CHAMPIONS: [ChampionInfo; 173] = [
    // 2009
    c("Alistar", "Alistar", "2009-02-21"),
    c("Annie", "Annie", "2009-02-21"),
    c("Ashe", "Ashe", "2009-02-21"),
    c("Fiddlesticks", "Fiddlesticks", "2009-02-21"),
    c("Jax", "Jax", "2009-02-21"),
    c("Kayle", "Kayle", "2009-02-21"),
    c("Master Yi", "MasterYi", "2009-02-21"),
    c("Morgana", "Morgana", "2009-02-21"),
    c("Nunu & Willump", "Nunu", "2009-02-21"),
    c("Ryze", "Ryze", "2009-02-21"),
    c("Sion", "Sion", "2009-02-21"),
    c("Sivir", "Sivir", "2009-02-21"),
    c("Soraka", "Soraka", "2009-02-21"),
    c("Teemo", "Teemo", "2009-02-21"),
    c("Tristana", "Tristana", "2009-02-21"),
    c("Twisted Fate", "TwistedFate", "2009-02-21"),
    c("Warwick", "Warwick", "2009-02-21"),
    c("Singed", "Singed", "2009-04-18"),
    c("Zilean", "Zilean", "2009-04-18"),
    c("Evelynn", "Evelynn", "2009-05-01"),
    c("Tryndamere", "Tryndamere", "2009-05-01"),
    c("Twitch", "Twitch", "2009-05-01"),
    c("Karthus", "Karthus", "2009-06-12"),
    c("Amumu", "Amumu", "2009-06-26"),
    c("Cho'Gath", "Chogath", "2009-06-26"),
    c("Anivia", "Anivia", "2009-07-10"),
    c("Rammus", "Rammus", "2009-07-10"),
    c("Veigar", "Veigar", "2009-07-24"),
    c("Kassadin", "Kassadin", "2009-08-07"),
    c("Gangplank", "Gangplank", "2009-08-19"),
    c("Taric", "Taric", "2009-08-19"),
    c("Blitzcrank", "Blitzcrank", "2009-09-02"),
    c("Dr. Mundo", "DrMundo", "2009-09-02"),
    c("Janna", "Janna", "2009-09-02"),
    c("Malphite", "Malphite", "2009-09-02"),
    c("Corki", "Corki", "2009-09-19"),
    c("Katarina", "Katarina", "2009-09-19"),
    c("Nasus", "Nasus", "2009-10-01"),
    c("Heimerdinger", "Heimerdinger", "2009-10-10"),
    c("Shaco", "Shaco", "2009-10-10"),
    c("Udyr", "Udyr", "2009-12-02"),
    c("Nidalee", "Nidalee", "2009-12-17"),
    // 2010
    c("Poppy", "Poppy", "2010-01-13"),
    c("Gragas", "Gragas", "2010-02-02"),
    c("Pantheon", "Pantheon", "2010-02-02"),
    c("Mordekaiser", "Mordekaiser", "2010-02-24"),
    c("Ezreal", "Ezreal", "2010-03-16"),
    c("Shen", "Shen", "2010-03-24"),
    c("Kennen", "Kennen", "2010-04-08"),
    c("Garen", "Garen", "2010-04-27"),
    c("Akali", "Akali", "2010-05-11"),
    c("Malzahar", "Malzahar", "2010-06-01"),
    c("Olaf", "Olaf", "2010-06-09"),
    c("Kog'Maw", "KogMaw", "2010-06-24"),
    c("Xin Zhao", "XinZhao", "2010-07-13"),
    c("Vladimir", "Vladimir", "2010-07-27"),
    c("Galio", "Galio", "2010-08-10"),
    c("Urgot", "Urgot", "2010-08-24"),
    c("Miss Fortune", "MissFortune", "2010-09-08"),
    c("Sona", "Sona", "2010-09-21"),
    c("Swain", "Swain", "2010-10-05"),
    c("Lux", "Lux", "2010-10-19"),
    c("LeBlanc", "Leblanc", "2010-11-02"),
    c("Irelia", "Irelia", "2010-11-16"),
    c("Trundle", "Trundle", "2010-12-01"),
    c("Cassiopeia", "Cassiopeia", "2010-12-14"),
    // 2011
    c("Caitlyn", "Caitlyn", "2011-01-04"),
    c("Renekton", "Renekton", "2011-01-18"),
    c("Karma", "Karma", "2011-02-01"),
    c("Maokai", "Maokai", "2011-02-16"),
    c("Jarvan IV", "JarvanIV", "2011-03-01"),
    c("Nocturne", "Nocturne", "2011-03-15"),
    c("Lee Sin", "LeeSin", "2011-04-01"),
    c("Brand", "Brand", "2011-04-12"),
    c("Rumble", "Rumble", "2011-04-26"),
    c("Vayne", "Vayne", "2011-05-10"),
    c("Orianna", "Orianna", "2011-06-01"),
    c("Yorick", "Yorick", "2011-06-22"),
    c("Leona", "Leona", "2011-07-13"),
    c("Wukong", "MonkeyKing", "2011-07-26"),
    c("Skarner", "Skarner", "2011-08-09"),
    c("Talon", "Talon", "2011-08-24"),
    c("Riven", "Riven", "2011-09-14"),
    c("Xerath", "Xerath", "2011-10-05"),
    c("Graves", "Graves", "2011-10-19"),
    c("Shyvana", "Shyvana", "2011-11-01"),
    c("Fizz", "Fizz", "2011-11-15"),
    c("Volibear", "Volibear", "2011-11-29"),
    c("Ahri", "Ahri", "2011-12-14"),
    c("Viktor", "Viktor", "2011-12-29"),
    // 2012
    c("Sejuani", "Sejuani", "2012-01-17"),
    c("Ziggs", "Ziggs", "2012-02-01"),
    c("Nautilus", "Nautilus", "2012-02-14"),
    c("Fiora", "Fiora", "2012-02-29"),
    c("Lulu", "Lulu", "2012-03-20"),
    c("Hecarim", "Hecarim", "2012-04-18"),
    c("Varus", "Varus", "2012-05-08"),
    c("Darius", "Darius", "2012-05-23"),
    c("Draven", "Draven", "2012-06-06"),
    c("Jayce", "Jayce", "2012-07-07"),
    c("Zyra", "Zyra", "2012-07-24"),
    c("Diana", "Diana", "2012-08-07"),
    c("Rengar", "Rengar", "2012-08-21"),
    c("Syndra", "Syndra", "2012-09-13"),
    c("Kha'Zix", "Khazix", "2012-09-27"),
    c("Elise", "Elise", "2012-10-26"),
    c("Zed", "Zed", "2012-11-13"),
    c("Nami", "Nami", "2012-12-07"),
    c("Vi", "Vi", "2012-12-19"),
    // 2013
    c("Thresh", "Thresh", "2013-01-23"),
    c("Quinn", "Quinn", "2013-03-01"),
    c("Zac", "Zac", "2013-03-29"),
    c("Lissandra", "Lissandra", "2013-04-30"),
    c("Aatrox", "Aatrox", "2013-06-13"),
    c("Lucian", "Lucian", "2013-08-22"),
    c("Jinx", "Jinx", "2013-10-10"),
    c("Yasuo", "Yasuo", "2013-12-13"),
    // 2014
    c("Vel'Koz", "Velkoz", "2014-02-27"),
    c("Braum", "Braum", "2014-05-12"),
    c("Gnar", "Gnar", "2014-08-14"),
    c("Azir", "Azir", "2014-09-16"),
    c("Kalista", "Kalista", "2014-11-20"),
    c("Rek'Sai", "RekSai", "2014-12-11"),
    // 2015
    c("Bard", "Bard", "2015-03-12"),
    c("Ekko", "Ekko", "2015-05-28"),
    c("Tahm Kench", "TahmKench", "2015-07-09"),
    c("Kindred", "Kindred", "2015-10-14"),
    c("Illaoi", "Illaoi", "2015-11-24"),
    // 2016
    c("Jhin", "Jhin", "2016-02-01"),
    c("Aurelion Sol", "AurelionSol", "2016-03-24"),
    c("Taliyah", "Taliyah", "2016-05-18"),
    c("Kled", "Kled", "2016-08-10"),
    c("Ivern", "Ivern", "2016-10-05"),
    c("Camille", "Camille", "2016-12-07"),
    // 2017
    c("Rakan", "Rakan", "2017-04-19"),
    c("Xayah", "Xayah", "2017-04-19"),
    c("Kayn", "Kayn", "2017-07-12"),
    c("Ornn", "Ornn", "2017-08-23"),
    c("Zoe", "Zoe", "2017-11-21"),
    // 2018
    c("Kai'Sa", "Kaisa", "2018-03-07"),
    c("Pyke", "Pyke", "2018-05-31"),
    c("Neeko", "Neeko", "2018-12-05"),
    // 2019
    c("Sylas", "Sylas", "2019-01-25"),
    c("Yuumi", "Yuumi", "2019-05-14"),
    c("Qiyana", "Qiyana", "2019-06-28"),
    c("Senna", "Senna", "2019-11-10"),
    c("Aphelios", "Aphelios", "2019-12-11"),
    // 2020
    c("Sett", "Sett", "2020-01-14"),
    c("Lillia", "Lillia", "2020-07-22"),
    c("Yone", "Yone", "2020-08-06"),
    c("Samira", "Samira", "2020-09-21"),
    c("Seraphine", "Seraphine", "2020-10-29"),
    c("Rell", "Rell", "2020-12-10"),
    // 2021
    c("Viego", "Viego", "2021-01-21"),
    c("Gwen", "Gwen", "2021-04-15"),
    c("Akshan", "Akshan", "2021-07-22"),
    c("Vex", "Vex", "2021-09-23"),
    // 2022
    c("Zeri", "Zeri", "2022-01-20"),
    c("Renata Glasc", "Renata", "2022-02-17"),
    c("Bel'Veth", "Belveth", "2022-06-09"),
    c("Nilah", "Nilah", "2022-07-13"),
    c("K'Sante", "KSante", "2022-11-03"),
    // 2023
    c("Milio", "Milio", "2023-03-23"),
    c("Naafiri", "Naafiri", "2023-07-19"),
    c("Briar", "Briar", "2023-09-14"),
    c("Hwei", "Hwei", "2023-12-06"),
    // 2024
    c("Smolder", "Smolder", "2024-01-31"),
    c("Aurora", "Aurora", "2024-07-17"),
    c("Ambessa", "Ambessa", "2024-11-06"),
    // 2025
    c("Mel", "Mel", "2025-01-22"),
    c("Yunara", "Yunara", "2025-07-16"),
    c("Zaahen", "Zaahen", "2025-11-19"),
    // 2026
    c("Locke", "Locke", "2026-06-24"),
];

/// Node value of the champion at `index` in [`CHAMPIONS`].
pub fn value_of(index: usize) -> u8 {
    FIRST_CHAMPION_VALUE + index as u8
}

/// Champion stored under a node value, if the value is a champion.
pub fn by_value(value: u8) -> Option<&'static ChampionInfo> {
    let index = value.checked_sub(FIRST_CHAMPION_VALUE)?;
    CHAMPIONS.get(index as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_start_at_alistar() {
        assert_eq!(by_value(16).unwrap().name, "Alistar");
        assert_eq!(by_value(17).unwrap().name, "Annie");
        assert_eq!(by_value(18).unwrap().name, "Ashe");
        assert_eq!(by_value(19).unwrap().name, "Fiddlesticks");
        assert!(by_value(15).is_none());
        assert_eq!(value_of(CHAMPIONS.len() - 1), 188);
        assert!(by_value(189).is_none());
    }

    #[test]
    fn ordered_by_release_then_name() {
        for pair in CHAMPIONS.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(
                (a.released, a.name) < (b.released, b.name),
                "{} ({}) must come before {} ({})",
                a.name,
                a.released,
                b.name,
                b.released
            );
        }
    }

    #[test]
    fn every_champion_has_an_icon_and_every_icon_a_champion() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("champion_icons");
        let mut icons: Vec<String> = std::fs::read_dir(dir)
            .expect("champion_icons folder")
            .flatten()
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_suffix(".png")
                    .map(String::from)
            })
            .collect();
        let mut keys: Vec<String> = CHAMPIONS.iter().map(|c| c.key.to_string()).collect();
        icons.sort();
        keys.sort();
        assert_eq!(icons, keys);
    }
}
