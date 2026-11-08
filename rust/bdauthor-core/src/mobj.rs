//! MovieObject.bdmv (structure MOBJ) + desassembleur HDMV — equivalent de `mobj.py`.
use crate::reader::*;

pub const GRP_BRANCH: u32 = 0;
pub const GRP_CMP: u32 = 1;
pub const GRP_SET: u32 = 2;

// mots d'instruction releves dans la reference (voir nav.py)
pub const PLAY_PL_INSN: u32 = 0x2280_0000;
pub const JUMP_TITLE_INSN: u32 = 0x2181_0000;
/// JUMP_TITLE vers le titre contenu dans un registre (operande `dst` = numero du registre).
pub const JUMP_TITLE_REG_INSN: u32 = 0x2101_0000;
pub const JUMP_OBJECT_INSN: u32 = 0x2180_0000;
pub const GOTO_INSN: u32 = 0x2081_0000;
pub const NOP_INSN: u32 = 0;
pub const MOVE_RR_INSN: u32 = 0x5000_0001;
pub const MOVE_RI_INSN: u32 = 0x5040_0001;
pub const PLAY_PL_PM_INSN: u32 = 0x4282_0000;
/// Comparaison « registre == valeur immediate » : si elle est fausse, la commande suivante est sautee.
pub const CMP_EQ_RI_INSN: u32 = 0x4840_0200;
/// CMP « registre < immediat » : la commande suivante ne s'execute que si le test est vrai.
pub const CMP_LT_RI_INSN: u32 = 0x4840_0700;
/// ADD « registre += immediat ».
pub const ADD_RI_INSN: u32 = 0x5040_0003;
/// SET_STREAM (groupe SET, sous-groupe SETSYSTEM), deux operandes immediats.
pub const SET_STREAM_INSN: u32 = 0x51C0_0001;

/// Reglage des sous-titres (PSR2) : `Some(k)` affiche la piste PG k (0-based), `None` masque les sous-titres sans changer de
/// piste. Operande dst : bit 15 = numero de piste valide (bits 0-11, 1-based), bit 14 = affichage.
pub fn set_pg_stream(track: Option<usize>) -> Command {
    match track {
        Some(k) => Command::new(SET_STREAM_INSN, 0x8000 | 0x4000 | ((k as u32 + 1) & 0xfff), 0),
        None => Command::new(SET_STREAM_INSN, 0, 0),
    }
}

/// Reglage de la piste audio principale (PSR1) : piste `k` (0-based). Operande dst : bit 31 = numero d'audio valide,
/// bits 16-27 = numero 1-based ; les bits des sous-titres (15 et 14) restent a 0, donc les sous-titres ne changent pas.
/// Format releve dans la reference libbluray (hdmv_vm) ; non verifie sur lecteur materiel.
pub fn set_audio_stream(track: usize) -> Command {
    Command::new(SET_STREAM_INSN, 0x8000_0000 | (((track as u32 + 1) & 0xfff) << 16), 0)
}

fn branch_name(sub: u32, opt: u32) -> Option<&'static str> {
    Some(match (sub, opt) {
        (0, 0) => "NOP", (0, 1) => "GOTO", (0, 2) => "BREAK",
        (1, 0) => "JUMP_OBJECT", (1, 1) => "JUMP_TITLE", (1, 2) => "CALL_OBJECT", (1, 3) => "CALL_TITLE", (1, 4) => "RESUME",
        (2, 0) => "PLAY_PL", (2, 1) => "PLAY_PL_PI", (2, 2) => "PLAY_PL_PM", (2, 3) => "TERMINATE_PL", (2, 4) => "LINK_PI", (2, 5) => "LINK_MK",
        _ => return None,
    })
}
fn cmp_name(opt: u32) -> Option<&'static str> {
    Some(match opt { 1 => "BC", 2 => "EQ", 3 => "NE", 4 => "GE", 5 => "GT", 6 => "LE", 7 => "LT", _ => return None })
}
fn set_name(sub: u32, opt: u32) -> Option<&'static str> {
    Some(match (sub, opt) {
        (0, 1) => "MOVE", (0, 2) => "SWAP", (0, 3) => "ADD", (0, 4) => "SUB", (0, 5) => "MUL", (0, 6) => "DIV", (0, 7) => "MOD",
        (0, 8) => "RND", (0, 9) => "AND", (0, 10) => "OR", (0, 11) => "XOR", (0, 12) => "BITSET", (0, 13) => "BITCLR",
        (0, 14) => "SHL", (0, 15) => "SHR",
        (1, 1) => "SET_STREAM", (1, 2) => "SET_NV_TIMER", (1, 3) => "SET_BUTTON_PAGE", (1, 4) => "ENABLE_BUTTON",
        (1, 5) => "DISABLE_BUTTON", (1, 6) => "SET_SEC_STREAM", (1, 7) => "POPUP_OFF", (1, 8) => "STILL_ON",
        (1, 9) => "STILL_OFF", (1, 10) => "SET_OUTPUT_MODE", (1, 11) => "SET_STREAM_SS",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Command {
    pub insn: u32,
    pub dst: u32,
    pub src: u32,
}

impl Command {
    pub fn new(insn: u32, dst: u32, src: u32) -> Self {
        Command { insn, dst, src }
    }
    pub fn op_cnt(&self) -> u32 { bits(self.insn, 3, 29) }
    pub fn grp(&self) -> u32 { bits(self.insn, 2, 27) }
    pub fn sub_grp(&self) -> u32 { bits(self.insn, 3, 24) }
    pub fn imm_op1(&self) -> bool { bits(self.insn, 1, 23) != 0 }
    pub fn imm_op2(&self) -> bool { bits(self.insn, 1, 22) != 0 }
    pub fn branch_opt(&self) -> u32 { bits(self.insn, 4, 16) }
    pub fn cmp_opt(&self) -> u32 { bits(self.insn, 4, 8) }
    pub fn set_opt(&self) -> u32 { bits(self.insn, 5, 0) }

    pub fn parse(r: &mut Reader) -> PResult<Self> {
        Ok(Command { insn: r.u32()?, dst: r.u32()?, src: r.u32()? })
    }
    pub fn pack(&self) -> [u8; 12] {
        let mut o = [0u8; 12];
        o[..4].copy_from_slice(&self.insn.to_be_bytes());
        o[4..8].copy_from_slice(&self.dst.to_be_bytes());
        o[8..].copy_from_slice(&self.src.to_be_bytes());
        o
    }
    fn reg(v: u32) -> String {
        if v & 0x8000_0000 != 0 { format!("PSR{}", v & 0x7fff_ffff) } else { format!("r{v}") }
    }
    fn operands(&self) -> String {
        let mut ops = Vec::new();
        if self.op_cnt() >= 1 {
            ops.push(if self.imm_op1() { self.dst.to_string() } else { Self::reg(self.dst) });
        }
        if self.op_cnt() >= 2 {
            ops.push(if self.imm_op2() { self.src.to_string() } else { Self::reg(self.src) });
        }
        ops.join(", ")
    }
    pub fn text(&self) -> String {
        let name = match self.grp() {
            GRP_BRANCH => branch_name(self.sub_grp(), self.branch_opt())
                .map(String::from).unwrap_or_else(|| format!("BRANCH?{}/{}", self.sub_grp(), self.branch_opt())),
            GRP_CMP => format!("CMP_{}", cmp_name(self.cmp_opt()).map(String::from).unwrap_or_else(|| format!("?{}", self.cmp_opt()))),
            GRP_SET => set_name(self.sub_grp(), self.set_opt())
                .map(String::from).unwrap_or_else(|| format!("SET?{}/{}", self.sub_grp(), self.set_opt())),
            g => format!("GRP?{g}"),
        };
        format!("{name} {}", self.operands()).trim_end().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MovieObject {
    pub resume_intention: u8,
    pub menu_call_mask: u8,
    pub title_search_mask: u8,
    pub commands: Vec<Command>,
}

impl MovieObject {
    /// Objet « normal » (flags 1,0,0) avec les commandes donnees.
    pub fn new(commands: Vec<Command>) -> Self {
        MovieObject { resume_intention: 1, menu_call_mask: 0, title_search_mask: 0, commands }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MovieObjectBdmv {
    pub version: String,
    pub objects: Vec<MovieObject>,
}

impl MovieObjectBdmv {
    pub fn parse(data: &[u8]) -> PResult<Self> {
        let mut r = Reader::new(data);
        if r.raw(4)? != b"MOBJ" {
            return err("signature MOBJ absente");
        }
        let version = r.ascii(4)?;
        r.seek(40);
        r.u32()?; // longueur
        r.u32()?; // reserve
        let n = r.u16()?;
        let mut objects = Vec::new();
        for _ in 0..n {
            let flags = r.u16()? as u32;
            let ncmd = r.u16()?;
            let commands = (0..ncmd).map(|_| Command::parse(&mut r)).collect::<PResult<Vec<_>>>()?;
            objects.push(MovieObject { resume_intention: bits(flags, 1, 15) as u8, menu_call_mask: bits(flags, 1, 14) as u8,
                                       title_search_mask: bits(flags, 1, 13) as u8, commands });
        }
        Ok(MovieObjectBdmv { version, objects })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut body = Vec::new();
        body.zeros(4);
        body.p16(self.objects.len() as u16);
        for o in &self.objects {
            let flags = ((o.resume_intention as u16) << 15) | ((o.menu_call_mask as u16) << 14) | ((o.title_search_mask as u16) << 13);
            body.p16(flags);
            body.p16(o.commands.len() as u16);
            for c in &o.commands {
                body.extend_from_slice(&c.pack());
            }
        }
        let mut out = Vec::new();
        out.text("MOBJ");
        out.text(&self.version);
        out.zeros(32);
        out.p32(body.len() as u32);
        out.extend(body);
        out
    }

    pub fn dump(&self, only: Option<&[usize]>) -> String {
        let mut out = vec![format!("version {}, {} objets", self.version, self.objects.len())];
        for (i, o) in self.objects.iter().enumerate() {
            if let Some(s) = only {
                if !s.contains(&i) { continue; }
            }
            out.push(format!("objet {i}: resume={} menu_mask={} title_mask={}", o.resume_intention, o.menu_call_mask, o.title_search_mask));
            for (j, c) in o.commands.iter().enumerate() {
                out.push(format!("  {j:3}: {}", c.text()));
            }
        }
        out.join("\n")
    }
}
