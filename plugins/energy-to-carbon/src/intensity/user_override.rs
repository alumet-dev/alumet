use super::EmissionIntensityProvider;

pub struct OverrideIntensity(pub f64);


struct OverrideIntensity(f64);
impl EmissionIntensityProvider for OverrideIntensity {
    fn get_intensity(&self) -> anyhow::Result<f64> {
        Ok(self.0)
    }
}