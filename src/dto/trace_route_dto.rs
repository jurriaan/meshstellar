#[derive(Clone, Debug)]
pub struct TracerouteDto {
    pub from_id: u32,
    pub to_id: u32,
    pub is_response: bool,
    pub route: Vec<u32>,
}
