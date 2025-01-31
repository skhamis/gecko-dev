/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */
#ifndef mozilla_BounceTrackingProtection_h__
#define mozilla_BounceTrackingProtection_h__

#include "mozilla/Logging.h"
#include "mozilla/MozPromise.h"
#include "nsIBounceTrackingProtection.h"
#include "nsIClearDataService.h"

class nsIPrincipal;
class nsITimer;

namespace mozilla {

class BounceTrackingState;
class BounceTrackingStateGlobal;
class BounceTrackingProtectionStorage;
class OriginAttributes;

extern LazyLogModule gBounceTrackingProtectionLog;

class BounceTrackingProtection final : public nsIBounceTrackingProtection {
  NS_DECL_ISUPPORTS
  NS_DECL_NSIBOUNCETRACKINGPROTECTION

 public:
  static already_AddRefed<BounceTrackingProtection> GetSingleton();

  // This algorithm is called when detecting the end of an extended navigation.
  // This could happen if a user-initiated navigation is detected in process
  // navigation start for bounce tracking, or if the client bounce detection
  // timer expires after process response received for bounce tracking without
  // observing a client redirect.
  nsresult RecordStatefulBounces(BounceTrackingState* aBounceTrackingState);

  // Stores a user activation flag with a timestamp for the given principal.
  nsresult RecordUserActivation(nsIPrincipal* aPrincipal);

 private:
  BounceTrackingProtection();
  ~BounceTrackingProtection() = default;

  // Timer which periodically runs PurgeBounceTrackers.
  nsCOMPtr<nsITimer> mBounceTrackingPurgeTimer;

  // Storage for user agent globals.
  RefPtr<BounceTrackingProtectionStorage> mStorage;

  // Clear state for classified bounce trackers. To be called on an interval.
  using PurgeBounceTrackersMozPromise =
      MozPromise<nsTArray<nsCString>, nsresult, true>;
  RefPtr<PurgeBounceTrackersMozPromise> PurgeBounceTrackers();

  nsresult PurgeBounceTrackersForStateGlobal(
      BounceTrackingStateGlobal* aStateGlobal,
      const OriginAttributes& aOriginAttributes);

  // Pending clear operations are stored as ClearDataMozPromise, one per host.
  using ClearDataMozPromise = MozPromise<nsCString, uint32_t, true>;
  nsTArray<RefPtr<ClearDataMozPromise>> mClearPromises;

  // Wraps nsIClearDataCallback in MozPromise.
  class ClearDataCallback final : public nsIClearDataCallback {
   public:
    NS_DECL_ISUPPORTS
    NS_DECL_NSICLEARDATACALLBACK

    explicit ClearDataCallback(ClearDataMozPromise::Private* aPromise,
                               const nsACString& aHost)
        : mHost(aHost), mPromise(aPromise){};

   private:
    virtual ~ClearDataCallback() { mPromise->Reject(0, __func__); }

    nsCString mHost;
    RefPtr<ClearDataMozPromise::Private> mPromise;
  };
};

}  // namespace mozilla

#endif
