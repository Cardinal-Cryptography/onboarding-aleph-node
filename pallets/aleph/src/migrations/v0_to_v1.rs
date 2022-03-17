use crate::Config;
use frame_support::traits::PalletInfoAccess;
use frame_support::{generate_storage_alias, log};
use frame_support::{
    traits::{Get, StorageVersion},
    weights::Weight,
};
use sp_std::vec::Vec;

generate_storage_alias!(
    Aleph, SessionForValidatorsChange => Value<u32>
);

generate_storage_alias!(
    Aleph, Validators<T: Config> => Value<Vec<T::AccountId>>
);

pub fn migrate<T: Config, P: PalletInfoAccess>() -> Weight {
    log::info!(target: "pallet_aleph", "Running migration from STORAGE_VERSION 0 to 1");

    let mut writes = 0;

    match SessionForValidatorsChange::translate(|old: Option<Option<u32>>| -> Option<u32> {
        log::info!(target: "pallet_aleph", "Current storage value for SessionForValidatorsChange {:?}", old);
        match old {
            Some(Some(x)) => Some(x),
            _ => None,
        }
    }) {
        Ok(_) => {
            writes += 1;
            log::info!(target: "pallet_aleph", "Succesfully migrated storage for SessionForValidatorsChange");
        }
        Err(why) => {
            log::error!(target: "pallet_aleph", "Something went wrong during the migration of SessionForValidatorsChange {:?}", why);
        }
    };

    match Validators::<T>::translate(
        |old: Option<Option<Vec<T::AccountId>>>| -> Option<Vec<T::AccountId>> {
            log::info!(target: "pallet_aleph", "Current storage value for Validators {:?}", old);
            match old {
                Some(Some(x)) => Some(x),
                _ => None,
            }
        },
    ) {
        Ok(_) => {
            writes += 1;
            log::info!(target: "pallet_aleph", "Succesfully migrated storage for Validators");
        }
        Err(why) => {
            log::error!(target: "pallet_aleph", "Something went wrong during the migration of Validators storage {:?}", why);
        }
    };

    // store new version
    StorageVersion::new(1).put::<P>();
    writes += 1;

    T::DbWeight::get().reads(2) + T::DbWeight::get().writes(writes)
}