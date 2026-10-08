//! Archive representation of the portable microsecond clock.
use super::{SampleDuration, SampleTime};
use rkyv::rancor::Fallible;
use rkyv::with::{ArchiveWith, DeserializeWith, SerializeWith};
use rkyv::{Archive, Archived, Place};

pub struct Microseconds;

macro_rules! archive_clock {
    ($clock:ty) => {
        impl ArchiveWith<$clock> for Microseconds {
            type Archived = Archived<u32>;
            type Resolver = ();
            fn resolve_with(value: &$clock, _: (), out: Place<Self::Archived>) {
                value.as_ticks().resolve((), out);
            }
        }
        impl<S: Fallible + ?Sized> SerializeWith<$clock, S> for Microseconds {
            fn serialize_with(_: &$clock, _: &mut S) -> Result<(), S::Error> {
                Ok(())
            }
        }
        impl<D: Fallible + ?Sized> DeserializeWith<Archived<u32>, $clock, D> for Microseconds {
            fn deserialize_with(value: &Archived<u32>, _: &mut D) -> Result<$clock, D::Error> {
                Ok(<$clock>::from_ticks(value.to_native()))
            }
        }
    };
}
archive_clock!(SampleTime);
archive_clock!(SampleDuration);
