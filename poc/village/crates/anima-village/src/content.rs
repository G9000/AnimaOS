use crate::*;

pub(crate) fn phrase(language: Language, ja: (&str, &str, &str), zh: (&str, &str, &str)) -> Phrase {
    let (text, translation, reading) = match language {
        Language::Ja => ja,
        Language::Zh => zh,
    };
    Phrase {
        text: text.into(),
        translation: translation.into(),
        reading: reading.into(),
    }
}
pub(crate) fn intro_note(language: Language) -> Phrase {
    phrase(
        language,
        (
            "タヴィのお店で、私のパンを二つ受け取ってね。— ミラ",
            "Please collect my two loaves from Tavi's shop. — Mira",
            "Tavi no omise de, watashi no pan o futatsu uketotte ne. — Mira",
        ),
        (
            "请去塔维的店，帮我取两个面包。——米拉",
            "Please collect my two loaves from Tavi's shop. — Mira",
            "Qǐng qù Tǎwéi de diàn, bāng wǒ qǔ liǎng ge miànbāo. — Mǐlā",
        ),
    )
}
pub fn choice_intent(id: &str) -> Result<Intent, String> {
    let intent = match id {
        "collect_order" => Intent {
            order: Some(OrderClaim {
                recipient: Some("Mira".into()),
                quantity: Some(2),
            }),
            ..Default::default()
        },
        "deliver_order" => Intent {
            delivery: Some(OrderClaim {
                recipient: Some("Mira".into()),
                quantity: Some(2),
            }),
            ..Default::default()
        },
        "ask_supper" => Intent {
            ask_supper: true,
            ..Default::default()
        },
        "honey" => Intent {
            preference: Some(Preference::Honey),
            ..Default::default()
        },
        "sesame" => Intent {
            preference: Some(Preference::Sesame),
            ..Default::default()
        },
        "ask_recipe" => Intent {
            ask_recipe: true,
            ..Default::default()
        },
        "help" => Intent {
            help: true,
            ..Default::default()
        },
        _ => return Err("Unknown reviewed choice".into()),
    };
    Ok(intent)
}
pub fn suggestions(state: &GameState) -> Vec<Suggestion> {
    if state.npc_id == NpcId::Mira {
        return crate::day_content::mira_suggestions(state);
    }
    let choices = [
        (
            "collect_order",
            phrase(
                state.language,
                (
                    "ミラのパンを二つ、お願いします。",
                    "I'd like Mira's two loaves, please.",
                    "Mira no pan o futatsu, onegai shimasu.",
                ),
                (
                    "我来取米拉的两个面包。",
                    "I'm here for Mira's two loaves.",
                    "Wǒ lái qǔ Mǐlā de liǎng ge miànbāo.",
                ),
            ),
        ),
        (
            "honey",
            phrase(
                state.language,
                (
                    "はちみつのパンが好きです。",
                    "I like honey bread.",
                    "Hachimitsu no pan ga suki desu.",
                ),
                (
                    "我喜欢蜂蜜面包。",
                    "I like honey bread.",
                    "Wǒ xǐhuan fēngmì miànbāo.",
                ),
            ),
        ),
        (
            "sesame",
            phrase(
                state.language,
                (
                    "ごまのパンが好きです。",
                    "I like sesame bread.",
                    "Goma no pan ga suki desu.",
                ),
                (
                    "我喜欢芝麻面包。",
                    "I like sesame bread.",
                    "Wǒ xǐhuan zhīma miànbāo.",
                ),
            ),
        ),
        (
            "ask_recipe",
            phrase(
                state.language,
                (
                    "新しいレシピはどうですか。",
                    "How is the new recipe going?",
                    "Atarashii reshipi wa dō desu ka.",
                ),
                (
                    "新食谱怎么样？",
                    "How is the new recipe going?",
                    "Xīn shípǔ zěnmeyàng?",
                ),
            ),
        ),
        (
            "help",
            phrase(
                state.language,
                (
                    "きっとおいしいですよ。応援しています。",
                    "I'm sure it'll be delicious. I'm cheering for you.",
                    "Kitto oishii desu yo. Ōen shite imasu.",
                ),
                (
                    "一定很好吃，我支持你。",
                    "I'm sure it'll be delicious. I'm cheering for you.",
                    "Yídìng hěn hǎochī, wǒ zhīchí nǐ.",
                ),
            ),
        ),
    ];
    choices
        .into_iter()
        .filter(|(id, _)| *id != "collect_order" || !state.has_collected())
        .map(|(id, p)| Suggestion {
            id: id.into(),
            text: p.text,
            translation: p.translation,
        })
        .collect()
}
pub fn reviewed_dialogue(state: &GameState, outcome: Outcome) -> Dialogue {
    if let Some(dialogue) = crate::day_content::reviewed_day(state, outcome) {
        return dialogue;
    }
    let p=match outcome {
        Outcome::Welcome if state.visits>1 && state.bread_preference==Some(Preference::Honey)=>phrase(state.language,
            ("おかえりなさい！好きなはちみつのパンを、お皿に用意したよ。","Welcome back! I put your favorite honey bread on the counter plate.","Okaeri nasai! Suki na hachimitsu no pan o, osara ni yōi shita yo."),
            ("欢迎回来！你喜欢的蜂蜜面包已经放在柜台的盘子里了。","Welcome back! Your favorite honey bread is on the counter plate.","Huānyíng huílái! Nǐ xǐhuan de fēngmì miànbāo yǐjīng fàng zài guìtái de pánzi lǐ le.")),
        Outcome::Welcome if state.visits>1 && state.bread_preference==Some(Preference::Sesame)=>phrase(state.language,
            ("おかえりなさい！好きなごまのパンを、お皿に用意したよ。","Welcome back! I put your favorite sesame bread on the counter plate.","Okaeri nasai! Suki na goma no pan o, osara ni yōi shita yo."),
            ("欢迎回来！你喜欢的芝麻面包已经放在柜台的盘子里了。","Welcome back! Your favorite sesame bread is on the counter plate.","Huānyíng huílái! Nǐ xǐhuan de zhīma miànbāo yǐjīng fàng zài guìtái de pánzi lǐ le.")),
        Outcome::Welcome if state.visits>1 && state.loaves==2=>phrase(state.language,
            ("おかえりなさい！今日は、はちみつとごまのパンがあるよ。","Welcome back! Today we have both honey and sesame bread.","Okaeri nasai! Kyō wa, hachimitsu to goma no pan ga aru yo."),
            ("欢迎回来！今天蜂蜜和芝麻面包都有。","Welcome back! Today we have both honey and sesame bread.","Huānyíng huílái! Jīntiān fēngmì hé zhīma miànbāo dōu yǒu.")),
        Outcome::Welcome=>phrase(state.language,
            ("いらっしゃい！ミラのパンを二つ、取りに来たの？はちみつとごま、どちらもあるよ。","Welcome! Are you here for Mira's two loaves? We have both honey and sesame bread.","Irasshai! Mira no pan o futatsu, tori ni kita no? Hachimitsu to goma, dochira mo aru yo."),
            ("欢迎！你来取米拉的两个面包吗？蜂蜜和芝麻口味都有。","Welcome! Are you here for Mira's two loaves? We have both honey and sesame bread.","Huānyíng! Nǐ lái qǔ Mǐlā de liǎng ge miànbāo ma? Fēngmì hé zhīma kǒuwèi dōu yǒu.")),
        Outcome::Collected=>phrase(state.language,
            ("はい、ミラのパンを二つと、お礼の二十コインです。新しいレシピが、少し心配なんだ。","Here are Mira's two loaves and twenty coins to thank you. I'm a little nervous about my new recipe.","Hai, Mira no pan o futatsu to, orei no nijū koin desu. Atarashii reshipi ga, sukoshi shinpai nan da."),
            ("给你，米拉的两个面包和二十枚金币。我在试新食谱，有点担心呢。","Here are Mira's two loaves and twenty coins to thank you. I'm a little nervous about my new recipe.","Gěi nǐ, Mǐlā de liǎng ge miànbāo hé èrshí méi jīnbì. Wǒ zài shì xīn shípǔ, yǒudiǎn dānxīn ne.")),
        Outcome::AlreadyCollected=>phrase(state.language,
            ("ミラのパンは、もう二つ渡したよ。かごを見てね。","I already gave you Mira's two loaves. Check your basket.","Mira no pan wa, mō futatsu watashita yo. Kago o mite ne."),
            ("米拉的两个面包已经给你了，看看篮子吧。","I already gave you Mira's two loaves. Check your basket.","Mǐlā de liǎng ge miànbāo yǐjīng gěi nǐ le, kànkan lánzi ba.")),
        Outcome::ClarifyOrder=>phrase(state.language,
            ("だれのパンを、いくつかな？メモには、ミラのパンを二つと書いてあるよ。","Whose bread, and how many? The note says two loaves for Mira.","Dare no pan o, ikutsu kana? Memo ni wa, Mira no pan o futatsu to kaite aru yo."),
            ("请问是谁的面包，要几个？纸条上写的是米拉的两个面包。","Whose bread, and how many? The note says two loaves for Mira.","Qǐngwèn shì shéi de miànbāo, yào jǐ ge? Zhǐtiáo shàng xiě de shì Mǐlā de liǎng ge miànbāo.")),
        Outcome::Preference=>match state.bread_preference {
            Some(Preference::Honey)=>phrase(state.language,("はちみつのパンが好きなんだね。覚えておくよ！","You like honey bread. I'll remember!","Hachimitsu no pan ga suki nan da ne. Oboete oku yo!"),("原来你喜欢蜂蜜面包，我会记住的！","You like honey bread. I'll remember!","Yuánlái nǐ xǐhuan fēngmì miànbāo, wǒ huì jìzhù de!")),
            _=>phrase(state.language,("ごまのパンが好きなんだね。覚えておくよ！","You like sesame bread. I'll remember!","Goma no pan ga suki nan da ne. Oboete oku yo!"),("原来你喜欢芝麻面包，我会记住的！","You like sesame bread. I'll remember!","Yuánlái nǐ xǐhuan zhīma miànbāo, wǒ huì jìzhù de!")),
        },
        Outcome::Recipe=>phrase(state.language,
            ("新しいレシピを考えているんだけど……おいしくできるか、少し心配なんだ。","I'm working on a new recipe... I'm a little nervous about how it'll turn out.","Atarashii reshipi o kangaete iru n da kedo... Oishiku dekiru ka, sukoshi shinpai nan da."),
            ("我在试一个新食谱……有点担心做得不好吃。","I'm trying a new recipe... I'm a little nervous it won't taste good.","Wǒ zài shì yí ge xīn shípǔ... Yǒudiǎn dānxīn zuò de bù hǎochī.")),
        Outcome::Help=>phrase(state.language,
            ("ありがとう。応援してくれて、少し勇気が出たよ。また来てね。","Thank you. Your encouragement helped me feel braver. Come back soon.","Arigatō. Ōen shite kurete, sukoshi yūki ga deta yo. Mata kite ne."),
            ("谢谢你的鼓励，我有信心多了。下次再来呀。","Thank you for encouraging me. I feel braver now. Visit again soon.","Xièxie nǐ de gǔlì, wǒ yǒu xìnxīn duō le. Xià cì zài lái ya.")),
        Outcome::Delivered | Outcome::AlreadyDelivered | Outcome::NeedBread | Outcome::ClarifyDelivery
        | Outcome::WrongSpeaker | Outcome::Supper | Outcome::EveningCallback => unreachable!(),
        Outcome::Talk=>phrase(state.language,
            ("ゆっくり話そう。パンの注文や、好きな味を教えてね。","Let's take our time. Tell me about the bread order or the flavor you like.","Yukkuri hanasō. Pan no chūmon ya, suki na aji o oshiete ne."),
            ("慢慢说吧。你可以告诉我面包的订单，或者喜欢的口味。","Take your time. Tell me about the order or the flavor you like.","Mànmàn shuō ba. Nǐ kěyǐ gàosu wǒ miànbāo de dìngdān, huòzhě xǐhuan de kǒuwèi.")),
    };
    let mut text = p.text;
    let mut translation = p.translation;
    let mut reading = p.reading;
    if outcome == Outcome::Welcome && state.visits > 1 && state.loaves == 2 {
        let callback = phrase(
            state.language,
            (
                "ミラのパンは、もう二つ受け取ったね。",
                "You already collected Mira's two loaves.",
                "Mira no pan wa, mō futatsu uketotta ne.",
            ),
            (
                "米拉的两个面包，你已经取走了。",
                "You already collected Mira's two loaves.",
                "Mǐlā de liǎng ge miànbāo, nǐ yǐjīng qǔ zǒu le.",
            ),
        );
        text.push_str(&format!(" {}", callback.text));
        translation.push_str(&format!(" {}", callback.translation));
        reading.push_str(&format!(" {}", callback.reading));
    }
    if outcome == Outcome::Welcome && state.visits > 1 && state.helped_recipe {
        let callback = phrase(
            state.language,
            (
                "この前の応援、うれしかったよ。",
                "Your encouragement last time meant a lot.",
                "Kono mae no ōen, ureshikatta yo.",
            ),
            (
                "上次你的鼓励让我很开心。",
                "Your encouragement last time meant a lot.",
                "Shàng cì nǐ de gǔlì ràng wǒ hěn kāixīn.",
            ),
        );
        text.push_str(&format!(" {}", callback.text));
        translation.push_str(&format!(" {}", callback.translation));
        reading.push_str(&format!(" {}", callback.reading));
    }
    Dialogue {
        text,
        translation,
        reading,
        expression: match outcome {
            Outcome::Collected | Outcome::Help | Outcome::Preference => Expression::Happy,
            Outcome::Recipe | Outcome::ClarifyOrder => Expression::Thoughtful,
            _ => Expression::Warm,
        },
        source: DialogueSource::Reviewed,
        suggestions: suggestions(state),
    }
}
