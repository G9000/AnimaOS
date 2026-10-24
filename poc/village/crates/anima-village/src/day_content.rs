use crate::content::phrase;
use crate::*;

pub(crate) fn mira_suggestions(state: &GameState) -> Vec<Suggestion> {
    let mut choices = vec![];
    if state.phase == Phase::Day {
        choices.push((
            "deliver_order",
            phrase(
                state.language,
                (
                    "ミラさん、パンを二つ届けに来ました。",
                    "Mira, I brought your two loaves.",
                    "Mira-san, pan o futatsu todoke ni kimashita.",
                ),
                (
                    "米拉，我来送你的两个面包。",
                    "Mira, I brought your two loaves.",
                    "Mǐlā, wǒ lái sòng nǐ de liǎng ge miànbāo.",
                ),
            ),
        ));
    }
    choices.push((
        "ask_supper",
        phrase(
            state.language,
            (
                "夕ごはんは何ですか。",
                "What's for supper?",
                "Yūgohan wa nan desu ka.",
            ),
            ("晚饭吃什么？", "What's for supper?", "Wǎnfàn chī shénme?"),
        ),
    ));
    choices
        .into_iter()
        .map(|(id, p)| Suggestion {
            id: id.into(),
            text: p.text,
            translation: p.translation,
        })
        .collect()
}

pub(crate) fn reviewed_day(state: &GameState, outcome: Outcome) -> Option<Dialogue> {
    let mut p = match outcome {
        Outcome::Welcome if state.npc_id == NpcId::Tavi && state.phase == Phase::Complete => phrase(state.language,
            ("おかえりなさい！村の夕ごはん、楽しみだね。ゆっくり話そう。","Welcome back! I'm looking forward to the village supper. Let's take our time.","Okaeri nasai! Mura no yūgohan, tanoshimi da ne. Yukkuri hanasō."),
            ("欢迎回来！很期待村里的晚饭，我们慢慢聊吧。","Welcome back! I'm looking forward to the village supper. Let's take our time.","Huānyíng huílái! Hěn qīdài cūn lǐ de wǎnfàn, wǒmen mànmàn liáo ba.")),
        Outcome::EveningCallback => phrase(state.language,
            ("ミラに届けてくれて、ありがとう！今夜の夕ごはんに、私の新しいパンも出すよ。",
             "Thank you for delivering the bread to Mira! My new recipe will be part of tonight's supper.",
             "Mira ni todokete kurete, arigatō! Kon'ya no yūgohan ni, watashi no atarashii pan mo dasu yo."),
            ("谢谢你把面包送给米拉！今晚的晚饭也会有我的新食谱面包。",
             "Thank you for delivering the bread to Mira! My new recipe will be part of tonight's supper.",
             "Xièxie nǐ bǎ miànbāo sòng gěi Mǐlā! Jīnwǎn de wǎnfàn yě huì yǒu wǒ de xīn shípǔ miànbāo.")),
        Outcome::Delivered => phrase(state.language,
            ("パンを二つ、ありがとう！これで村の夕ごはんの用意ができるよ。タヴィにも伝えてね。",
             "Thank you for both loaves! Now I can prepare the village supper. Please tell Tavi they arrived.",
             "Pan o futatsu, arigatō! Kore de mura no yūgohan no yōi ga dekiru yo. Tavi ni mo tsutaete ne."),
            ("谢谢你送来两个面包！现在可以准备村里的晚饭了。也告诉塔维一声吧。",
             "Thank you for both loaves! Now I can prepare the village supper. Please tell Tavi they arrived.",
             "Xièxie nǐ sòng lái liǎng ge miànbāo! Xiànzài kěyǐ zhǔnbèi cūn lǐ de wǎnfàn le. Yě gàosu Tǎwéi yì shēng ba.")),
        Outcome::AlreadyDelivered => phrase(state.language,
            ("パンはもう二つ受け取ったよ。夕ごはんの用意は順調だよ。","I already received both loaves. Supper preparations are going well.","Pan wa mō futatsu uketotta yo. Yūgohan no yōi wa junchō da yo."),
            ("两个面包已经收到了，晚饭准备得很顺利。","I already received both loaves. Supper preparations are going well.","Liǎng ge miànbāo yǐjīng shōudào le, wǎnfàn zhǔnbèi de hěn shùnlì.")),
        Outcome::AlreadyCollected if state.phase != Phase::Day => phrase(state.language,
            ("ミラのパンはもう渡したね。新しい注文はないよ。","Mira's bread has already been delivered. There isn't another order.","Mira no pan wa mō watashita ne. Atarashii chūmon wa nai yo."),
            ("米拉的面包已经送到了，没有新的订单。","Mira's bread has already been delivered. There isn't another order.","Mǐlā de miànbāo yǐjīng sòngdào le, méiyǒu xīn de dìngdān.")),
        Outcome::NeedBread => phrase(state.language,
            ("タヴィのお店で、私のパンを二つ受け取ってから来てね。","Please collect my two loaves from Tavi's shop first.","Tavi no omise de, watashi no pan o futatsu uketotte kara kite ne."),
            ("请先去塔维的店取我的两个面包。","Please collect my two loaves from Tavi's shop first.","Qǐng xiān qù Tǎwéi de diàn qǔ wǒ de liǎng ge miànbāo.")),
        Outcome::ClarifyDelivery => phrase(state.language,
            ("だれに、いくつ届けるの？私の注文はパン二つだよ。","Who is the delivery for, and how many? My order is for two loaves.","Dare ni, ikutsu todokeru no? Watashi no chūmon wa pan futatsu da yo."),
            ("这是给谁的，要送几个？我订了两个面包。","Who is the delivery for, and how many? My order is for two loaves.","Zhè shì gěi shéi de, yào sòng jǐ ge? Wǒ dìng le liǎng ge miànbāo.")),
        Outcome::WrongSpeaker if state.npc_id == NpcId::Mira => phrase(state.language,
            ("パンを受け取るなら、タヴィのお店へどうぞ。私はここで夕ごはんを準備しているよ。","Please collect the bread at Tavi's shop. I'm preparing supper here.","Pan o uketoru nara, Tavi no omise e dōzo. Watashi wa koko de yūgohan o junbi shite iru yo."),
            ("取面包请去塔维的店，我在这里准备晚饭。","Please collect the bread at Tavi's shop. I'm preparing supper here.","Qǔ miànbāo qǐng qù Tǎwéi de diàn, wǒ zài zhèlǐ zhǔnbèi wǎnfàn.")),
        Outcome::WrongSpeaker => phrase(state.language,
            ("届け先はミラだよ。広場で夕ごはんの準備をしているよ。","Please deliver those loaves to Mira. She's preparing supper in the square.","Todokesaki wa Mira da yo. Hiroba de yūgohan no junbi o shite iru yo."),
            ("请把面包送给米拉，她在广场准备晚饭。","Please deliver those loaves to Mira. She's preparing supper in the square.","Qǐng bǎ miànbāo sòng gěi Mǐlā, tā zài guǎngchǎng zhǔnbèi wǎnfàn.")),
        Outcome::Supper => phrase(state.language,
            ("今夜は野菜のスープとタヴィのパンだよ。みんなで食べようね。","Tonight we're having vegetable soup and Tavi's bread. Let's eat together.","Kon'ya wa yasai no sūpu to Tavi no pan da yo. Minna de tabeyō ne."),
            ("今晚吃蔬菜汤和塔维的面包，大家一起吃吧。","Tonight we're having vegetable soup and Tavi's bread. Let's eat together.","Jīnwǎn chī shūcài tāng hé Tǎwéi de miànbāo, dàjiā yìqǐ chī ba.")),
        _ if state.npc_id == NpcId::Mira && state.phase != Phase::Day => return reviewed_day(state, Outcome::AlreadyDelivered),
        _ if state.npc_id == NpcId::Mira => phrase(state.language,
            ("こんにちは、ミラです。夕ごはんのために、タヴィのパンを二つ待っているよ。","Hello, I'm Mira. I'm waiting for two loaves from Tavi for supper.","Konnichiwa, Mira desu. Yūgohan no tame ni, Tavi no pan o futatsu matte iru yo."),
            ("你好，我是米拉。我在等塔维的两个面包，准备晚饭用。","Hello, I'm Mira. I'm waiting for two loaves from Tavi for supper.","Nǐ hǎo, wǒ shì Mǐlā. Wǒ zài děng Tǎwéi de liǎng ge miànbāo, zhǔnbèi wǎnfàn yòng.")),
        _ => return None,
    };
    if outcome == Outcome::EveningCallback {
        let preference = match state.bread_preference {
            Some(Preference::Honey) => Some(phrase(
                state.language,
                (
                    "好きなはちみつのパンも用意したよ。",
                    "I saved your favorite honey bread too.",
                    "Suki na hachimitsu no pan mo yōi shita yo.",
                ),
                (
                    "也准备了你喜欢的蜂蜜面包。",
                    "I saved your favorite honey bread too.",
                    "Yě zhǔnbèi le nǐ xǐhuan de fēngmì miànbāo.",
                ),
            )),
            Some(Preference::Sesame) => Some(phrase(
                state.language,
                (
                    "好きなごまのパンも用意したよ。",
                    "I saved your favorite sesame bread too.",
                    "Suki na goma no pan mo yōi shita yo.",
                ),
                (
                    "也准备了你喜欢的芝麻面包。",
                    "I saved your favorite sesame bread too.",
                    "Yě zhǔnbèi le nǐ xǐhuan de zhīma miànbāo.",
                ),
            )),
            None => None,
        };
        if let Some(extra) = preference {
            append(&mut p, extra);
        }
        if state.helped_recipe {
            append(
                &mut p,
                phrase(
                    state.language,
                    (
                        "応援してくれて、勇気が出たよ。",
                        "Your encouragement helped me feel brave.",
                        "Ōen shite kurete, yūki ga deta yo.",
                    ),
                    (
                        "你的鼓励给了我勇气。",
                        "Your encouragement helped me feel brave.",
                        "Nǐ de gǔlì gěi le wǒ yǒngqì.",
                    ),
                ),
            );
        }
    }
    Some(Dialogue {
        text: p.text,
        translation: p.translation,
        reading: p.reading,
        expression: Expression::Happy,
        source: DialogueSource::Reviewed,
        suggestions: suggestions(state),
    })
}
fn append(p: &mut Phrase, extra: Phrase) {
    p.text.push_str(&format!(" {}", extra.text));
    p.translation.push_str(&format!(" {}", extra.translation));
    p.reading.push_str(&format!(" {}", extra.reading));
}
