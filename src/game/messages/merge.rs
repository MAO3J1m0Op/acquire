use std::fmt;

use serde::{Deserialize, Serialize};

use crate::game::{Company, CompanyMap};

/// Indicates a merger in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Merge {
    /// The smaller companies that are being removed from the board by the
    /// merging process. The companies are ordered such that the defunct company
    /// at position `0` is the last one to be resolved, so companies are ordered
    /// largest to smallest.
    defunct: [Option<Company>; 3],
    /// The company into which the defunct company is merging.
    pub into: Company,
}

impl Merge {

    /// Creates a new merge that specifies that the list of `defunct` companies
    /// will go `into` the provided company in the provided order.
    pub fn new(defunct: &[Company], into: Company) -> Self {
        assert!(defunct.len() > 0, "merge created with empty defunct");
        assert!(defunct.len() <= 3,
            "merge created with more than 3 defunct companies"
        );
        let mut defunct_arr = [None; 3];
        for (i, &company) in defunct.iter().rev().enumerate() {
            defunct_arr[i] = Some(company);
        }

        Merge { defunct: defunct_arr, into }
    }

    /// Iterates through defunct companies in the order that they will be resolved.
    pub fn defunct(&self) -> impl Iterator<Item = Company> + '_ {
        self.defunct.iter().filter_map(|opt| *opt).rev()
    }

    /// Removes a defunct company from the list, returning [`None`] if there are
    /// no defunct companies left in the merge.
    pub fn pop_defunct(&mut self) -> Option<Company> {
        for i in (0..3).rev() {
            if self.defunct[i].is_some() {
                return Some(self.defunct[i].take().unwrap());
            }
        }

        None
    }

    pub fn defunct_is_empty(&self) -> bool {
        self.defunct.iter().all(|cmp| cmp.is_none())
    }

    /// Creates a correct merge given the participants and their sizes on the
    /// board. Returns an [`Err`] variant if there are ties that must be
    /// resolved by the player.
    ///
    /// # Invariants
    ///
    /// * `participants` should have at least 2 true entries.
    pub fn make_merge(
        participants: CompanyMap<bool>,
        company_sizes: CompanyMap<u8>,
    ) -> Result<Self, MergeTie> {

        let mut participants = participants.true_companies();
        // Sort in descending order by size
        participants.sort_by_key(|&cmp| company_sizes[cmp]);
        participants.reverse();

        let first_tie = find_first_tie(&participants, company_sizes);

        // No ties
        let Some(first_tie) = first_tie else {

            // Put the defunct back in ascending order, as that's how we wish to resolve the merge
            let defunct = &mut participants[1..];
            defunct.reverse();
            let defunct = &participants[1..];

            // Unwrap: participants is length 2 at minimum by invariants
            let prevailing = *participants.first().unwrap();

            return Ok(Merge::new(defunct, prevailing));
        };

        // Prevailing tie
        if first_tie.end == 0 {
            let candidates = &participants[first_tie.clone()];
            let other_participants = &participants[first_tie.end+1..];
            return Err(MergeTie::prevailing(
                CompanyMap::collect_included(candidates.iter().copied()),
                CompanyMap::collect_included(other_participants.iter().copied()),
            ));
        }

        // Either a defunct tie or no tie
        let prevailing = participants[0];
        let defunct = &mut participants[1..];
        defunct.reverse();

        Self::make_merge_with_prevailing(defunct, prevailing, company_sizes)
    }

    /// Makes a merge with the prevailing chosen already.
    ///
    /// # Invariants
    ///
    /// * `sorted_defunct` must be sorted in *ascending* order with respect to
    ///   company sizes, and have minimum length 1 and maximum length 3.
    fn make_merge_with_prevailing(
        sorted_defunct: &[Company],
        prevailing: Company,
        company_sizes: CompanyMap<u8>,
    ) -> Result<Merge, MergeTie> {

        let first_tie = find_first_tie(&sorted_defunct, company_sizes);

        // No tie
        let Some(first_tie) = first_tie else {
            return Ok(Merge::new(sorted_defunct, prevailing));
        };

        let tie_struct = match first_tie.len() {
            0 | 1 => panic!("this case already handled above"),
            2 => {
                // Is the tie or the other company larger?
                if first_tie.start == 0 {

                    // Then the other company is larger, if it exists
                    let other = sorted_defunct.last().copied();

                    MergeDefunctTie::TwoWay {
                        tied_companies: [
                            // We know these to exist and be part of the tie
                            sorted_defunct[0],
                            sorted_defunct[1]
                        ],
                        third_defunct: other,
                        third_defunct_larger: true,
                    }
                }

                else {
                    // The tie start must be 1
                    debug_assert_eq!(first_tie.start, 1);
                    MergeDefunctTie::TwoWay {
                        tied_companies: [
                            // There must be a tie, and it's not in the first index,
                            // so defunct[1] and defunct[2] must be defined
                            sorted_defunct[1],
                            sorted_defunct[2],
                        ],
                        third_defunct: Some(sorted_defunct[0]),
                        third_defunct_larger: false,
                    }
                }
            },
            3 => {
                MergeDefunctTie::ThreeWay {
                    tie: [
                        sorted_defunct[0],
                        sorted_defunct[1],
                        sorted_defunct[2],
                    ]
                }
            },
            _ => panic!("more than 3 defunct elements in a merge"),
        };

        Err(MergeTie::defunct(
            prevailing,
            tie_struct,
        ))
    }
}


/// A tie in constructing a merge that requires resolution by the player.
pub struct MergeTie {
    tie: MergeTieImpl
}

/// A tie in constructing a merge that requires resolution by the player. I
/// don't export the enum because I don't want to export [`MergeTie`]'s internal
/// structure.
#[derive(Debug)]
enum MergeTieImpl {
    Prevailing {
        /// Must have 2 true entries
        candidates: CompanyMap<bool>,
        other_participants: CompanyMap<bool>,
    },
    Defunct {
        /// Must not be included anywhere in the tie structure.
        prevailing: Company,
        tie: MergeDefunctTie,
    }
}

impl fmt::Debug for MergeTie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.tie, f)
    }
}

impl MergeTie {
    /// Constructor for prevailing tie variant
    fn prevailing(candidates: CompanyMap<bool>, other_participants: CompanyMap<bool>) -> Self {
        Self {
            tie: MergeTieImpl::Prevailing {
                candidates,
                other_participants,
            },
        }
    }

    /// Constructor for defunct tie variant
    fn defunct(prevailing: Company, tie: MergeDefunctTie) -> Self {
        Self {
            tie: MergeTieImpl::Defunct {
                prevailing,
                tie,
            }
        }
    }

    /// Returns a [`CompanyMap`] marking all the participants in the chosen tie.
    pub fn participants(&self) -> CompanyMap<bool> {
        match &self.tie {
            MergeTieImpl::Prevailing { candidates, other_participants: _ } => {
                *candidates
            },
            MergeTieImpl::Defunct { prevailing: _, tie } => {
                match tie {
                    MergeDefunctTie::TwoWay { tied_companies, third_defunct: _, third_defunct_larger: _ } => {
                        CompanyMap::collect_included(tied_companies.iter().copied())
                    },
                    MergeDefunctTie::ThreeWay { tie } => {
                        CompanyMap::collect_included(tie.iter().copied())
                    },
                }
            },
        }
    }

    /// Advances the tie by supplying a company chosen by the player to break
    /// the tie.
    /// * In a prevailing tie, the company chosen will become the prevailing
    ///   company.
    /// * In a defunct tie, the company chosen will be resolved first.
    ///
    /// # Panics
    ///
    /// `chosen_company` must be participating in the tie. This function will
    /// panic if that is not the case.
    pub fn advance(self, chosen_company: Company, company_sizes: CompanyMap<u8>) -> Result<Merge, Self> {
        match self.tie {
            MergeTieImpl::Prevailing { mut candidates, other_participants } => {
                let prevailing = chosen_company;

                // Remove the prevailing company from the list of candidates
                assert!(candidates[prevailing]);
                candidates[prevailing] = false;

                let defunct_candidates = candidates.or(other_participants);

                // Get a sorted list of companies by size
                let mut defunct_candidates = defunct_candidates.true_companies();
                defunct_candidates.sort_by_key(|&company| company_sizes[company]);

                Merge::make_merge_with_prevailing(&defunct_candidates, prevailing, company_sizes)
            },
            MergeTieImpl::Defunct { prevailing, tie } => {
                match tie {
                    MergeDefunctTie::TwoWay {
                        tied_companies,
                        third_defunct,
                        third_defunct_larger
                    } => {
                        let idx_of_chosen = tied_companies
                            .iter()
                            .position(|&tie| tie == chosen_company)
                            // unwrap: chosen should be in tie per panic condition
                            .unwrap();

                        let idx_of_other = match idx_of_chosen {
                            0 => 1,
                            1 => 0,
                            _ => panic!("tied_companies not of size 2"),
                        };
                        let other = tied_companies[idx_of_other];

                        // Use this information to determine the order of merge
                        let defunct: &[Company] = if let Some(third_defunct) = third_defunct {
                            if third_defunct_larger {
                                &[
                                    chosen_company,
                                    other,
                                    third_defunct,
                                ]
                            } else {
                                &[
                                    third_defunct,
                                    chosen_company,
                                    other,
                                ]
                            }
                        } else {
                            &[
                                chosen_company,
                                other,
                            ]
                        };

                        Ok(Merge::new(defunct, prevailing))
                    }
                    MergeDefunctTie::ThreeWay { tie } => {

                        let idx_of_chosen = tie
                            .iter()
                            .position(|&tie| tie == chosen_company)
                            // unwrap: chosen should be in tie per panic condition
                            .unwrap();

                        // Get index of two companies still tied
                        let idx_a = match idx_of_chosen {
                            0 => 1,
                            1 | 2 => 0,
                            _ => panic!("tie not of size 3"),
                        };
                        let idx_b = match idx_of_chosen {
                            0 | 1 => 2,
                            2 => 1,
                            _ => panic!("tie not of size 3"),
                        };

                        // The two companies that are still tied
                        let tie_a = tie[idx_a];
                        let tie_b = tie[idx_b];

                        Err(MergeTie::defunct(prevailing, MergeDefunctTie::TwoWay {
                            tied_companies: [tie_a, tie_b],
                            third_defunct: Some(chosen_company),
                            third_defunct_larger: false,
                        }))
                    },
                }
            },
        }
    }
}

/// A tie in the defunct companies.
#[derive(Debug)]
enum MergeDefunctTie {
    TwoWay {
        /// Must be distinct
        tied_companies: [Company; 2],
        /// Must be distinct from `tied_companies` if `Some`
        third_defunct: Option<Company>,
        third_defunct_larger: bool,
    },
    ThreeWay {
        /// Must be distinct
        tie: [Company; 3],
    }
}

/// Searches the list sorted by size and tries to find any companies who are the
/// same size. Returns the start and end index of the first tie found. Returns
/// [`None`] if no ties are found.
fn find_first_tie(
    sorted_participants: &[Company],
    company_sizes: CompanyMap<u8>
) -> Option<std::ops::Range<usize>> {

    let mut tie_start: Option<usize> = None;
    let mut tie_end: usize = 0;

    // Get the first index out on its own
    let mut iter = sorted_participants.iter().enumerate();
    let mut prev: Company = *iter.next()?.1;

    for (i, &company) in iter {
        let prev_size = company_sizes[prev];
        let curr_size = company_sizes[prev];

        // Do we find a tie?
        if prev_size == curr_size {

            // Have we started a tie yet?
            if tie_start.is_none() {
                // Record the previous index as the start of the tie.
                tie_start = Some(i - 1);
            }

            tie_end = i;
        }

        else {
            // Did this break an existing tie? If so, we're done
            if tie_start.is_some() { break; }
        }

        // Reset previous to keep the loop invariant
        prev = company;
    }

    // Return None if we never started a tie
    let tie_start = tie_start?;

    // Return the tie index
    Some(tie_start..tie_end)
}

#[cfg(test)]
mod test {
    use crate::game::{Company::*, CompanyMap};

    use super::find_first_tie;


    #[test]
    pub fn test_find_first_tie() {

        let participants = &[
            Imperial,
            Festival,
            Continental,
            Worldwide,
        ];
        let mut sizes = CompanyMap::new(&0);

        // no tie
        sizes[Imperial] = 4;
        sizes[Festival] = 3;
        sizes[Continental] = 2;
        sizes[Worldwide] = 1;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, None);

        // tie at beginning
        sizes[Imperial] = 4;
        sizes[Festival] = 4;
        sizes[Continental] = 2;
        sizes[Worldwide] = 1;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, Some(0..2));

        // tie in middle
        sizes[Imperial] = 4;
        sizes[Festival] = 3;
        sizes[Continental] = 3;
        sizes[Worldwide] = 1;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, Some(1..3));

        // tie in end
        sizes[Imperial] = 4;
        sizes[Festival] = 3;
        sizes[Continental] = 2;
        sizes[Worldwide] = 2;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, Some(2..4));

        // two ties
        sizes[Imperial] = 4;
        sizes[Festival] = 4;
        sizes[Continental] = 2;
        sizes[Worldwide] = 2;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, Some(0..2));

        // 3 way tie (beginning)
        sizes[Imperial] = 4;
        sizes[Festival] = 4;
        sizes[Continental] = 4;
        sizes[Worldwide] = 2;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, Some(0..3));

        // 3 way tie (end)
        sizes[Imperial] = 5;
        sizes[Festival] = 4;
        sizes[Continental] = 4;
        sizes[Worldwide] = 4;
        let result = find_first_tie(participants, sizes);
        assert_eq!(result, Some(1..4));
    }
}
