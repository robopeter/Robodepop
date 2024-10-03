use nih_plug::prelude::*;
use robo_depop_plugin::Gain;

pub fn main() {
    nih_export_standalone::<Gain>();
}
