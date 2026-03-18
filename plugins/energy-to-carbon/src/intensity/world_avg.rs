// imports



struct WorldAvgIntensity;
impl EmissionIntensityProvider for WorldAvgIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(475.0)
    }
}
